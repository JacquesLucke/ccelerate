// SPDX-License-Identifier: MIT

#include <CLI/CLI.hpp>
#include <algorithm>
#include <clang/AST/ASTConsumer.h>
#include <clang/AST/DeclCXX.h>
#include <clang/AST/DeclTemplate.h>
#include <clang/AST/NestedNameSpecifier.h>
#include <clang/AST/TypeLoc.h>
#include <clang/ASTMatchers/ASTMatchFinder.h>
#include <clang/ASTMatchers/ASTMatchers.h>
#include <clang/Basic/DiagnosticSema.h>
#include <clang/Basic/Version.h>
#include <clang/CodeGen/CodeGenAction.h>
#include <clang/Driver/Compilation.h>
#include <clang/Driver/Driver.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Frontend/CompilerInvocation.h>
#include <clang/Frontend/FrontendAction.h>
#include <clang/Frontend/FrontendOptions.h>
#include <clang/Frontend/TextDiagnosticBuffer.h>
#include <clang/Frontend/TextDiagnosticPrinter.h>
#include <clang/FrontendTool/Utils.h>
#include <clang/Lex/HeaderSearchOptions.h>
#include <clang/Lex/Lexer.h>
#include <clang/Lex/MacroInfo.h>
#include <clang/Lex/PPCallbacks.h>
#include <clang/Lex/Preprocessor.h>
#include <clang/Rewrite/Core/Rewriter.h>
#include <clang/Tooling/CommonOptionsParser.h>
#include <clang/Tooling/Tooling.h>
#include <ctre.hpp>
#include <filesystem>
#include <fmt/format.h>
#include <llvm/Config/llvm-config.h>
#include <llvm/Support/CommandLine.h>
#include <llvm/Support/MemoryBuffer.h>
#include <llvm/Support/TargetSelect.h>
#include <llvm/Support/VirtualFileSystem.h>
#include <llvm/TargetParser/Host.h>
#include <unordered_map>
#include <unordered_set>

#include "clang_for_ccelerate_io.hh"
#include "config.hh"
#include "error_code.hh"
#include "get_current_executable_path.hh"
#include "local_code_info.hh"
#include "memory.hh"
#include "span.hh"
#include "string.hh"

namespace ccelerate {

struct Cmd_ParseArgs {
  path cwd;
  string binary;
  vector<string> clang_args;
};

struct Cmd_CompileObj {
  vector<string> clang_args;
};

struct Cmd_ExtractLocalCode {
  vector<string> clang_args;
  path local_code_path;
  string local_id = "__";
  vector<path> config_paths;
  vector<PathMap> path_maps;
};

struct Cmd_CompileLocalCode {
  vector<string> clang_args;
  vector<path> local_code_paths;
  path obj_output_path;
};

static int handle__parse_args(const Cmd_ParseArgs &args) {
  const path self_path = get_current_executable_path();
  std::string clang_path = self_path.parent_path().parent_path() /
                           "vcpkg_installed/x64-linux/tools/llvm" / args.binary;
  std::string libstdcxx_path =
      self_path.parent_path().parent_path().parent_path().parent_path() /
      "extern" / "libstdc++";

  vector<const char *> driver_args;
  driver_args.push_back(clang_path.c_str());
  driver_args.push_back("-stdlib=libstdc++");
  const string toolchain_arg =
      fmt::format("--gcc-toolchain={}", libstdcxx_path);
  driver_args.push_back(toolchain_arg.c_str());
  for (const string &arg : args.clang_args) {
    driver_args.push_back(arg.c_str());
  }

  clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_ops =
      new clang::DiagnosticOptions();
  clang::TextDiagnosticPrinter *diag_client =
      new clang::TextDiagnosticPrinter(llvm::errs(), &*diag_ops);
  clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
      new clang::DiagnosticIDs());
  clang::DiagnosticsEngine diags(diag_id, diag_ops, diag_client);

  std::string target_triple = llvm::sys::getProcessTriple();
  clang::driver::Driver driver(
      clang_path, target_triple, diags, "clang LLVM Compiler");
  std::unique_ptr<clang::driver::Compilation> compilation{
      driver.BuildCompilation(driver_args)};
  if (!compilation) {
    fmt::println(stderr, "Failed to build compilation");
    return 1;
  }
  if (compilation->containsError()) {
    fmt::println(stderr, "Compilation contains error");
    return 1;
  }
  if (driver.getDiags().hasErrorOccurred()) {
    return 1;
  }
  const auto &jobs = compilation->getJobs();
  if (jobs.empty()) {
    return 0;
  }
  fwrite(clang_io::magic.data(), clang_io::magic.size(), 1, stdout);
  clang_io::ParsedArgs parsed_args;
  for (auto &job : jobs) {
    clang_io::Command command;
    command.kind = job.getSource().getKind();
    command.executable = job.getExecutable();
    for (const auto &arg : job.getArguments()) {
      command.args.push_back(arg);
    }
    for (const clang::driver::InputInfo &input_info : job.getInputInfos()) {
      clang_io::InputInfo io_input;
      io_input.type = input_info.getType();
      if (const char *filename = input_info.getFilename()) {
        io_input.filename = string(filename);
      }
      command.input_infos.push_back(std::move(io_input));
    }
    for (const string &output_files : job.getOutputFilenames()) {
      command.output_files.push_back(output_files);
    }
    parsed_args.commands.push_back(command);
  }

  msgpack::sbuffer buffer;
  msgpack::pack(buffer, parsed_args);
  fwrite(buffer.data(), buffer.size(), 1, stdout);
  return 0;
}

static int execute_cc1(const vector<string> &clang_args) {
  // The driver job args start with "-cc1"; CompilerInvocation expects the
  // remaining cc1 options only (same as clang's cc1_main entry point).
  vector<const char *> cc1_args;
  cc1_args.reserve(clang_args.size());
  for (const string &arg : clang_args) {
    if (cc1_args.empty() && arg == "-cc1") {
      continue;
    }
    cc1_args.push_back(arg.c_str());
  }

  llvm::InitializeAllTargets();
  llvm::InitializeAllTargetMCs();
  llvm::InitializeAllAsmPrinters();
  llvm::InitializeAllAsmParsers();

  clang::CompilerInstance clang_instance;
  clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
      new clang::DiagnosticIDs());
  clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_opts(
      new clang::DiagnosticOptions());
  clang::TextDiagnosticBuffer *diags_buffer = new clang::TextDiagnosticBuffer();
  clang::DiagnosticsEngine diags(diag_id, diag_opts, diags_buffer);

  bool success = clang::CompilerInvocation::CreateFromArgs(
      clang_instance.getInvocation(), cc1_args, diags);

  clang_instance.createDiagnostics();
  if (!clang_instance.hasDiagnostics()) {
    return 1;
  }
  diags_buffer->FlushDiagnostics(clang_instance.getDiagnostics());
  if (!success) {
    return 1;
  }

  success = clang::ExecuteCompilerInvocation(&clang_instance);
  return success ? 0 : 1;
}

static int handle__compile_obj(const Cmd_CompileObj &args) {
  return execute_cc1(args.clang_args);
}

static vector<string_view> split_into_lines(const string_view src) {
  vector<string_view> lines;
  size_t start = 0;
  while (true) {
    const size_t end = src.find('\n', start);
    if (end == string_view::npos) {
      lines.push_back(src.substr(start));
      break;
    }
    lines.push_back(src.substr(start, end - start));
    start = end + 1;
  }
  return lines;
}

struct LineMarker {
  int line_number;
  string_view file;
  bool is_start_of_new_file;
  bool is_return_to_file;
  bool is_system_header;
  bool is_extern_c;

  static optional<LineMarker> parse(const string_view line) {
    const ctre::regex_results match =
        ctre::match<R"!(# (\d+) "(.*)"\s*(\d?)\s*(\d?)\s*(\d?)\s*(\d?))!">(
            line);
    if (!match) {
      return nullopt;
    }
    const optional<string_view> line_number = match.get<1>().to_optional_view();
    const optional<string_view> file = match.get<2>().to_optional_view();
    vector<int> flags;
    if (const optional<string_view> f = match.get<3>().to_optional_view()) {
      if (!f->empty()) {
        flags.push_back(std::stoi(string(*f)));
      }
    }
    if (const optional<string_view> f = match.get<4>().to_optional_view()) {
      if (!f->empty()) {
        flags.push_back(std::stoi(string(*f)));
      }
    }
    if (const optional<string_view> f = match.get<5>().to_optional_view()) {
      if (!f->empty()) {
        flags.push_back(std::stoi(string(*f)));
      }
    }
    if (const optional<string_view> f = match.get<6>().to_optional_view()) {
      if (!f->empty()) {
        flags.push_back(std::stoi(string(*f)));
      }
    }
    auto flag_is_set = [&](const int flag) {
      return std::ranges::any_of(flags, [&](const int f) { return f == flag; });
    };
    return LineMarker{
        .line_number = std::stoi(string(*line_number)),
        .file = *file,
        .is_start_of_new_file = flag_is_set(1),
        .is_return_to_file = flag_is_set(2),
        .is_system_header = flag_is_set(3),
        .is_extern_c = flag_is_set(4),
    };
  }
};

static string_view trim_whitespace(const string_view str) {
  const size_t start = str.find_first_not_of(" \t\n\r");
  if (start == string_view::npos) {
    return {};
  }
  const size_t end = str.find_last_not_of(" \t\n\r");
  return str.substr(start, end - start + 1);
}

struct LocalCodeState {
  const Cmd_ExtractLocalCode &args;
  const Config &config;
  llvm::BumpPtrAllocator alloc;

  path object_path;

  clang::InputKind input_kind;
  string raw_preprocessed;
  string code_for_parser;
  vector<string_view> local_code_lines;
  vector<int> map_parser_to_local_lines;
  // Include order needs to be preserved.
  std::unordered_map<path, size_t> direct_include_index;
  vector<LocalCodeInfo::DirectInclude> direct_includes;

  std::unordered_set<string> seen_local_define_names;
  std::vector<string> include_defines;

  vector<LocalCodeInfo::UsingNamespace> using_namespaces;

  optional<clang::Rewriter> rewriter;
  string rewrite_result;
};

static string format_define_line(const clang::IdentifierInfo &identifier,
                                 const clang::MacroInfo &macro,
                                 clang::Preprocessor &preprocessor) {
  string result = "#define ";
  result += identifier.getName();
  if (macro.isFunctionLike()) {
    result += '(';
    const llvm::ArrayRef<const clang::IdentifierInfo *> params = macro.params();
    for (size_t param_i = 0; param_i < params.size(); param_i++) {
      const clang::IdentifierInfo &param = *params[param_i];
      if (param.getName() == "__VA_ARGS__") {
        result += "...";
      } else {
        result += param.getName();
      }
      if (param_i + 1 != params.size()) {
        result += ',';
      }
    }
    if (macro.isGNUVarargs()) {
      result += "...";
    }
    result += ')';
  }

  llvm::SmallVector<clang::Token, 16> expanded_tokens;
  if (!macro.tokens_empty()) {
    llvm::SmallVector<clang::Token, 16> stream_tokens(macro.tokens().begin(),
                                                      macro.tokens().end());
    clang::Token eof;
    eof.startToken();
    eof.setKind(clang::tok::eof);
    eof.setLocation(macro.getDefinitionEndLoc());
    stream_tokens.push_back(eof);

    preprocessor.EnterTokenStream(stream_tokens,
                                  /*DisableMacroExpansion=*/false,
                                  /*IsReinject=*/false);
    while (true) {
      clang::Token tok;
      preprocessor.Lex(tok);
      if (tok.is(clang::tok::eof)) {
        break;
      }
      expanded_tokens.push_back(tok);
    }
  }

  if (!expanded_tokens.empty()) {
    result += ' ';
    llvm::SmallString<128> spelling_buffer;
    bool first = true;
    for (const clang::Token &tok : expanded_tokens) {
      if (!first && tok.hasLeadingSpace()) {
        result += ' ';
      }
      result += preprocessor.getSpelling(tok, spelling_buffer);
      first = false;
    }
  }
  return result;
}

class IncludeDefineCallbacks : public clang::PPCallbacks {
private:
  LocalCodeState &state_;
  clang::Preprocessor &preprocessor_;

  bool location_is_in_local_code(clang::SourceLocation loc) const {
    clang::SourceManager &sm = preprocessor_.getSourceManager();
    const clang::SourceLocation spelling = sm.getSpellingLoc(loc);
    if (sm.isWrittenInMainFile(spelling)) {
      return true;
    }
    const clang::OptionalFileEntryRef file =
        sm.getFileEntryRefForID(sm.getFileID(spelling));
    if (!file) {
      return false;
    }
    return state_.config.is_local_header(path(file->getName().str()));
  }

public:
  IncludeDefineCallbacks(LocalCodeState &state,
                         clang::Preprocessor &preprocessor)
      : state_(state), preprocessor_(preprocessor) {}

  void MacroDefined(const clang::Token &MacroNameTok,
                    const clang::MacroDirective *MD) override {
    const clang::IdentifierInfo *identifier = MacroNameTok.getIdentifierInfo();
    if (!identifier ||
        !state_.config.is_include_define(identifier->getName())) {
      return;
    }
    if (!this->location_is_in_local_code(MacroNameTok.getLocation())) {
      return;
    }
    const clang::MacroInfo *macro = MD->getMacroInfo();
    if (!macro || macro->isBuiltinMacro()) {
      return;
    }
    state_.seen_local_define_names.insert(string(identifier->getName()));
    state_.include_defines.push_back(
        format_define_line(*identifier, *macro, preprocessor_));
  }

  void MacroUndefined(const clang::Token &MacroNameTok,
                      const clang::MacroDefinition & /*MD*/,
                      const clang::MacroDirective * /*Undef*/) override {
    const clang::IdentifierInfo *identifier = MacroNameTok.getIdentifierInfo();
    if (!identifier ||
        !state_.config.is_include_define(identifier->getName())) {
      return;
    }
    if (!this->location_is_in_local_code(MacroNameTok.getLocation())) {
      return;
    }
    if (state_.seen_local_define_names.contains(
            string(identifier->getName()))) {
      return;
    }
    state_.include_defines.push_back(
        fmt::format("#undef {}", identifier->getName().str()));
  }
};

class ExtractPreprocessedLocalCodeAction
    : public clang::PreprocessorFrontendAction {
private:
  LocalCodeState &state_;

public:
  ExtractPreprocessedLocalCodeAction(LocalCodeState &state) : state_(state) {}

  void ExecuteAction() override {
    clang::CompilerInstance &compiler = this->getCompilerInstance();
    clang::Preprocessor &pp = compiler.getPreprocessor();
    pp.addPPCallbacks(std::make_unique<IncludeDefineCallbacks>(state_, pp));

    llvm::raw_string_ostream os(state_.raw_preprocessed);
    clang::PreprocessorOutputOptions opts;
    opts.ShowCPP = true;
    opts.ShowLineMarkers = true;
    // opts.ShowMacros = true;
    clang::DoPrintPreprocessedInput(pp, &os, opts);
    os.flush();

    vector<string_view> header_stack;
    int local_depth = 0;

    llvm::StringSaver saver(state_.alloc);

    std::unordered_set<string_view> all_includes;
    int committed_kept_lines = 0;

    const vector<string_view> lines = split_into_lines(state_.raw_preprocessed);
    for (size_t line_i = 0; line_i < lines.size(); line_i++) {
      const bool is_local = int(header_stack.size()) == local_depth;
      const bool line_only_whitespace = trim_whitespace(lines[line_i]).empty();
      const string_view line = lines[line_i];
      if (line.starts_with("# ")) {
        const optional<LineMarker> line_marker = LineMarker::parse(line);
        if (!line_marker) {
          continue;
        }
        const string_view file = line_marker->file;
        if (line_marker->is_start_of_new_file) {
          if (is_local) {
            if (state_.config.is_local_header(file)) {
              local_depth++;
            } else {
              if (file != "<built-in>") {
                const path include_path(file);
                const auto [it, inserted] = state_.direct_include_index.emplace(
                    include_path, state_.direct_includes.size());
                const bool is_pure_c =
                    state_.config.is_pure_c_header(include_path);
                if (inserted) {
                  state_.direct_includes.push_back(LocalCodeInfo::DirectInclude{
                      .include_path = include_path,
                      .is_system_header = line_marker->is_system_header,
                      .is_pure_c = is_pure_c,
                  });
                } else {
                  state_.direct_includes[it->second].is_system_header |=
                      line_marker->is_system_header;
                  state_.direct_includes[it->second].is_pure_c |= is_pure_c;
                }
              }
            }
          }
          all_includes.insert(file);
          header_stack.push_back(file);
        } else if (line_marker->is_return_to_file) {
          header_stack.pop_back();
          local_depth = std::min<int>(local_depth, header_stack.size());
        }
        if (int(header_stack.size()) == local_depth) {
          state_.local_code_lines.resize(committed_kept_lines);
          const string_view out_line_marker = saver.save(
              fmt::format("# {} \"{}\"", line_marker->line_number, file));
          state_.local_code_lines.push_back(out_line_marker);
        }
        state_.map_parser_to_local_lines.push_back(-1);
        state_.code_for_parser += line;
        state_.code_for_parser += '\n';
      } else {
        if (is_local) {
          state_.local_code_lines.push_back(line);
          if (!line_only_whitespace) {
            committed_kept_lines = state_.local_code_lines.size();
          }
        }
        if (!line_only_whitespace) {
          state_.map_parser_to_local_lines.push_back(
              is_local ? state_.local_code_lines.size() - 1 : -1);
          state_.code_for_parser += line;
          state_.code_for_parser += '\n';
        }
      }
    }
    // No trailing line marker to trigger the usual trim of whitespace-only
    // lines (including the empty entry from a final '\n' in the preprocess).
    state_.local_code_lines.resize(committed_kept_lines);
    // Final newline mapping.
    state_.map_parser_to_local_lines.push_back(-1);
  }
};

static bool mapped_location_is_local(const LocalCodeState &state,
                                     clang::SourceLocation loc,
                                     const clang::SourceManager &sm) {
  if (!loc.isValid()) {
    return false;
  }
  const size_t line = sm.getExpansionLineNumber(loc);
  if (line == 0 || line > state.map_parser_to_local_lines.size()) {
    return false;
  }
  return state.map_parser_to_local_lines[line - 1] != -1;
}

// True when the token at the location literally spells expected string. False
// for an implicit ctor of an anonymous union: getNameAsString() is
// "(anonymous union at file:line:col)" while `loc` points at the `union`
// keyword.
static bool token_spells_name(clang::SourceLocation loc,
                              llvm::StringRef expected,
                              const clang::SourceManager &sm,
                              const clang::LangOptions &lang_opts) {
  if (!loc.isValid()) {
    return false;
  }
  const uint32_t tok_len = clang::Lexer::MeasureTokenLength(loc, sm, lang_opts);
  const llvm::StringRef tok_spelling(sm.getCharacterData(loc), tok_len);
  return tok_spelling == expected;
}

// Walk past linkage-specs etc.; reject function/class scope.
static const clang::DeclContext *
enclosing_file_context(const clang::DeclContext *dc) {
  while (dc) {
    if (dc->isFunctionOrMethod() || dc->isRecord()) {
      return nullptr;
    }
    if (dc->isFileContext()) {
      return dc;
    }
    dc = dc->getParent();
  }
  return nullptr;
}

static string format_used_namespace(const clang::UsingDirectiveDecl &ud,
                                    const clang::PrintingPolicy &policy) {
  string result;
  llvm::raw_string_ostream os(result);
  if (const clang::NestedNameSpecifier *qualifier = ud.getQualifier()) {
    qualifier->print(os, policy);
  }
  if (const clang::NamedDecl *nominated = ud.getNominatedNamespaceAsWritten()) {
    os << nominated->getName();
  }
  os.flush();
  return result;
}

static string format_parent_namespace(const clang::DeclContext *file_context) {
  if (const auto *ns =
          llvm::dyn_cast_or_null<clang::NamespaceDecl>(file_context)) {
    return ns->getQualifiedNameAsString();
  }
  return {};
}

class UsingNamespaceCollector
    : public clang::ast_matchers::MatchFinder::MatchCallback {
private:
  LocalCodeState &state_;
  clang::SourceManager &sm_;

public:
  UsingNamespaceCollector(LocalCodeState &state)
      : state_(state), sm_(state.rewriter->getSourceMgr()) {}

  void
  run(const clang::ast_matchers::MatchFinder::MatchResult &result) override {
    const clang::UsingDirectiveDecl *ud =
        result.Nodes.getNodeAs<clang::UsingDirectiveDecl>("usingNamespace");
    if (!ud) {
      return;
    }
    if (!mapped_location_is_local(state_, ud->getLocation(), sm_)) {
      return;
    }
    const clang::DeclContext *file_context =
        enclosing_file_context(ud->getLexicalDeclContext());
    if (!file_context) {
      return;
    }
    const clang::PrintingPolicy policy(result.Context->getLangOpts());
    state_.using_namespaces.push_back(LocalCodeInfo::UsingNamespace{
        .parent = format_parent_namespace(file_context),
        .used = format_used_namespace(*ud, policy),
    });
  }
};

class SymbolRenamer : public clang::ast_matchers::MatchFinder::MatchCallback {
private:
  LocalCodeState &state_;
  clang::SourceManager &sm_;
  std::unordered_set<uint32_t> renamed_locs_;

public:
  SymbolRenamer(LocalCodeState &state)
      : state_(state), sm_(state.rewriter->getSourceMgr()) {}

  virtual void
  run(const clang::ast_matchers::MatchFinder::MatchResult &result) override {
    clang::SourceLocation loc;
    const clang::NamedDecl *named_decl = nullptr;

    if (const clang::NamedDecl *decl =
            result.Nodes.getNodeAs<clang::NamedDecl>("staticDecl")) {
      loc = decl->getLocation();
      named_decl = decl;
    } else if (const clang::DeclRefExpr *ref =
                   result.Nodes.getNodeAs<clang::DeclRefExpr>("declRef")) {
      loc = ref->getLocation();
      named_decl = ref->getDecl();
    } else if (const clang::UnresolvedLookupExpr *ule =
                   result.Nodes.getNodeAs<clang::UnresolvedLookupExpr>(
                       "unresolvedLookup")) {
      loc = ule->getNameLoc();
      for (clang::NamedDecl *candidate : ule->decls()) {
        const clang::FunctionDecl *fd = nullptr;
        if (const auto *f = llvm::dyn_cast<clang::FunctionDecl>(candidate)) {
          fd = f;
        } else if (const auto *ftd =
                       llvm::dyn_cast<clang::FunctionTemplateDecl>(candidate)) {
          fd = ftd->getTemplatedDecl();
        } else {
          return;
        }
        if (!this->function_overload_set_should_be_localized(*fd)) {
          return;
        }
        named_decl = fd;
      }
    } else if (const clang::TypeLoc *type_loc =
                   result.Nodes.getNodeAs<clang::TypeLoc>("tagTypeLoc")) {
      const clang::TagTypeLoc tag_tl =
          type_loc->getAsAdjusted<clang::TagTypeLoc>();
      if (tag_tl.isNull()) {
        return;
      }
      loc = tag_tl.getNameLoc();
      named_decl = tag_tl.getDecl();
    } else if (const clang::TypeLoc *type_loc =
                   result.Nodes.getNodeAs<clang::TypeLoc>(
                       "templateSpecTypeLoc")) {
      const clang::TemplateSpecializationTypeLoc tst =
          type_loc->getAsAdjusted<clang::TemplateSpecializationTypeLoc>();
      if (tst.isNull()) {
        return;
      }
      loc = tst.getTemplateNameLoc();
      const clang::TemplateName template_name =
          tst.getTypePtr()->getTemplateName();
      if (const clang::TemplateDecl *td = template_name.getAsTemplateDecl()) {
        if (const auto *ctd = llvm::dyn_cast<clang::ClassTemplateDecl>(td)) {
          named_decl = ctd->getTemplatedDecl();
        } else if (const auto *tatd =
                       llvm::dyn_cast<clang::TypeAliasTemplateDecl>(td)) {
          named_decl = tatd->getTemplatedDecl();
        }
      }
    } else if (const clang::TypeLoc *type_loc =
                   result.Nodes.getNodeAs<clang::TypeLoc>(
                       "deducedTemplateSpecTypeLoc")) {
      const clang::DeducedTemplateSpecializationTypeLoc dtst =
          type_loc
              ->getAsAdjusted<clang::DeducedTemplateSpecializationTypeLoc>();
      if (dtst.isNull()) {
        return;
      }
      loc = dtst.getTemplateNameLoc();
      const clang::TemplateName template_name =
          dtst.getTypePtr()->getTemplateName();
      if (const clang::TemplateDecl *td = template_name.getAsTemplateDecl()) {
        if (const auto *ctd = llvm::dyn_cast<clang::ClassTemplateDecl>(td)) {
          named_decl = ctd->getTemplatedDecl();
        } else if (const auto *tatd =
                       llvm::dyn_cast<clang::TypeAliasTemplateDecl>(td)) {
          named_decl = tatd->getTemplatedDecl();
        }
      }
    } else if (const clang::TypeLoc *type_loc =
                   result.Nodes.getNodeAs<clang::TypeLoc>("typedefTypeLoc")) {
      const clang::TypedefTypeLoc typedef_tl =
          type_loc->getAsAdjusted<clang::TypedefTypeLoc>();
      if (typedef_tl.isNull()) {
        return;
      }
      loc = typedef_tl.getNameLoc();
      named_decl = typedef_tl.getTypedefNameDecl();
    }
    if (!loc.isValid() || !named_decl) {
      return;
    }
    if (!this->name_should_be_localized(*named_decl)) {
      return;
    }
    const std::string old_name = named_decl->getNameAsString();
    if (old_name.empty()) {
      return;
    }
    if (!token_spells_name(
            loc, old_name, sm_, state_.rewriter->getLangOpts())) {
      return;
    }
    const uint32_t loc_key = loc.getRawEncoding();
    if (!renamed_locs_.insert(loc_key).second) {
      return;
    }
    state_.rewriter->InsertTextAfterToken(loc, state_.args.local_id);
  }

  // Individual function locality, ignoring other overloads of the same name.
  bool function_alone_should_be_localized(const clang::FunctionDecl &decl) {
    if (decl.isExternallyVisible()) {
      return false;
    }
    const clang::FunctionDecl *anchor = decl.getDefinition();
    if (!anchor) {
      anchor = decl.getCanonicalDecl();
    }
    const clang::DeclContext *dc = anchor->getDeclContext();
    if (dc->isFunctionOrMethod() || dc->isRecord()) {
      return false;
    }
    return this->location_is_in_local_code(anchor->getLocation());
  }

  // Don't rename a static overload if a non-localizable overload (e.g. an
  // external function template) shares the name: instantiation DeclRefExprs
  // would rewrite the shared call site and leave the template name behind.
  bool
  function_overload_set_should_be_localized(const clang::FunctionDecl &decl) {
    if (!this->function_alone_should_be_localized(decl)) {
      return false;
    }
    const clang::DeclarationName name = decl.getDeclName();
    if (!name.isIdentifier()) {
      return true;
    }
    const clang::DeclContext *dc = decl.getDeclContext()->getRedeclContext();
    for (const clang::NamedDecl *other : dc->lookup(name)) {
      if (const auto *other_fd = llvm::dyn_cast<clang::FunctionDecl>(other)) {
        if (!this->function_alone_should_be_localized(*other_fd)) {
          return false;
        }
      } else if (const auto *ftd =
                     llvm::dyn_cast<clang::FunctionTemplateDecl>(other)) {
        if (!this->function_alone_should_be_localized(
                *ftd->getTemplatedDecl())) {
          return false;
        }
      }
    }
    return true;
  }

  bool name_should_be_localized(const clang::NamedDecl &decl) {
    if (const auto *ctor = llvm::dyn_cast<clang::CXXConstructorDecl>(&decl)) {
      if (ctor->isInheritingConstructor()) {
        return false;
      }
      const clang::CXXRecordDecl *record = ctor->getParent();
      if (const clang::TagDecl *def = record->getDefinition()) {
        return this->name_should_be_localized(*def);
      }
      return false;
    }
    if (const auto *fd = llvm::dyn_cast<clang::FunctionDecl>(&decl)) {
      return this->function_overload_set_should_be_localized(*fd);
    }
    if (const auto *vd = llvm::dyn_cast<clang::VarDecl>(&decl)) {
      if (decl.isExternallyVisible()) {
        return false;
      }
      if (const clang::VarDecl *def = vd->getDefinition()) {
        const clang::DeclContext *dc = def->getDeclContext();
        if (dc->isFunctionOrMethod() || dc->isRecord()) {
          return false;
        }
        return this->location_is_in_local_code(def->getLocation());
      }
    }
    if (const auto *td = llvm::dyn_cast<clang::TagDecl>(&decl)) {
      if (const clang::TagDecl *def = td->getDefinition()) {
        const clang::DeclContext *dc = def->getDeclContext();
        if (dc->isFunctionOrMethod() || dc->isRecord()) {
          return false;
        }
        if (decl.isExternallyVisible()) {
          return state_.config.is_local_type(def->getQualifiedNameAsString());
        }
        return this->location_is_in_local_code(def->getLocation());
      }
    }
    if (const auto *ecd = llvm::dyn_cast<clang::EnumConstantDecl>(&decl)) {
      if (decl.isExternallyVisible()) {
        return false;
      }
      const auto *ed = llvm::cast<clang::EnumDecl>(ecd->getDeclContext());
      // Scoped enumerators are not injected into the enclosing scope, so
      // renaming the enum type is enough to avoid collisions.
      if (ed->isScoped()) {
        return false;
      }
      if (const clang::EnumDecl *def = ed->getDefinition()) {
        const clang::DeclContext *dc = def->getDeclContext();
        if (dc->isFunctionOrMethod() || dc->isRecord()) {
          return false;
        }
        return this->location_is_in_local_code(def->getLocation());
      }
    }
    if (const auto *td = llvm::dyn_cast<clang::TypedefNameDecl>(&decl)) {
      if (decl.isExternallyVisible()) {
        return state_.config.is_local_type(td->getQualifiedNameAsString());
      }
      if (!td->isInAnonymousNamespace()) {
        return false;
      }
      for (const auto &redecl : td->redecls()) {
        const clang::DeclContext *dc = redecl->getDeclContext();
        if (dc->isFunctionOrMethod() || dc->isRecord()) {
          return false;
        }
        if (!this->location_is_in_local_code(redecl->getLocation())) {
          return false;
        }
      }
      return true;
    }
    return false;
  }

  bool location_is_in_local_code(clang::SourceLocation loc) const {
    return mapped_location_is_local(state_, loc, sm_);
  }
};

// Checks if ADL is used in this call.
static bool is_adl_call_expr(const clang::DeclRefExpr &ref,
                             clang::ASTContext &ast) {
  if (ref.getQualifier()) {
    return false;
  }
  const clang::Stmt *current = &ref;
  while (current) {
    const clang::DynTypedNodeList parents = ast.getParents(*current);
    if (parents.empty()) {
      return false;
    }
    const clang::DynTypedNode &parent = parents[0];
    if (parent.get<clang::ImplicitCastExpr>() ||
        parent.get<clang::ParenExpr>()) {
      current = parent.get<clang::Stmt>();
      continue;
    }
    if (const auto *call = parent.get<clang::CallExpr>()) {
      return call->usesADL() &&
             call->getCallee()->IgnoreParenImpCasts() == &ref;
    }
    return false;
  }
  return false;
}

static string
generate_fully_qualified_name_for_rewrite(const clang::NamedDecl &decl,
                                          const clang::LangOptions &lang) {
  clang::PrintingPolicy policy(lang);
  policy.SuppressUnwrittenScope = true;
  string result = "::";
  llvm::raw_string_ostream os(result);
  decl.printQualifiedName(os, policy);
  os.flush();
  return result;
}

static bool try_qualify_type_nested_name_specifier(
    LocalCodeState &state,
    std::unordered_set<uint32_t> &edited_locs,
    clang::NestedNameSpecifierLoc nns_loc);

class CallQualifierInserter
    : public clang::ast_matchers::MatchFinder::MatchCallback {
  LocalCodeState &state_;
  clang::SourceManager &sm_;
  std::unordered_set<uint32_t> edited_locs_;

public:
  CallQualifierInserter(LocalCodeState &state)
      : state_(state), sm_(state.rewriter->getSourceMgr()) {}

  void
  run(const clang::ast_matchers::MatchFinder::MatchResult &result) override {
    if (const clang::DeclRefExpr *ref =
            result.Nodes.getNodeAs<clang::DeclRefExpr>("declRef")) {
      clang::SourceLocation loc = ref->getLocation();
      if (!mapped_location_is_local(state_, loc, sm_)) {
        return;
      }
      const clang::ValueDecl *decl = ref->getDecl();
      if (decl->isTemplateParameter()) {
        return;
      }
      // Qualify type prefixes on any DeclRef nested-name (including methods):
      // `Instance::StaticData::get` → `::ns::Instance::StaticData::get`.
      // TypeLoc matchers often miss a separate TagTypeLoc for the leading type
      // inside DeclRefExpr qualifiers (Clang stores one TypeSpec NNS).
      if (const clang::NestedNameSpecifierLoc qualifier =
              ref->getQualifierLoc()) {
        if (try_qualify_type_nested_name_specifier(
                state_, edited_locs_, qualifier)) {
          return;
        }
      }
      const std::string name = decl->getNameAsString();
      if (name.empty()) {
        return;
      }
      const clang::LangOptions &lang_opts = state_.rewriter->getLangOpts();
      if (!token_spells_name(loc, name, sm_, lang_opts)) {
        return;
      }
      // Unscoped enumerators are injected into the enum's enclosing context, so
      // DeclContext is the EnumDecl (not a file context). Qualify them so a
      // merged TU's `using namespace` that introduces a same-named type (e.g.
      // struct Const vs TokenType::Const) cannot hide the enumerator.
      // Scoped enumerators stay as `Enum::Enumerator`; the enum type prefix is
      // handled by TypeQualifierInserter.
      const clang::DeclContext *decl_ctx = decl->getDeclContext();
      if (const auto *ecd = llvm::dyn_cast<clang::EnumConstantDecl>(decl)) {
        const auto *ed = llvm::cast<clang::EnumDecl>(ecd->getDeclContext());
        if (ed->isScoped()) {
          return;
        }
        decl_ctx = ed->getDeclContext();
      }
      if (!decl_ctx->isFileContext()) {
        return;
      }
      if (is_adl_call_expr(*ref, *result.Context)) {
        return;
      }

      const string qualified =
          generate_fully_qualified_name_for_rewrite(*decl, lang_opts);

      // Only rewrite the qualifier + name and keep explicit template arguments.
      clang::SourceLocation begin = loc;
      if (const clang::NestedNameSpecifierLoc qualifier =
              ref->getQualifierLoc()) {
        begin = qualifier.getBeginLoc();
      }
      const clang::CharSourceRange range =
          clang::CharSourceRange::getTokenRange(begin, loc);
      if (!range.isValid()) {
        return;
      }
      const llvm::StringRef existing =
          clang::Lexer::getSourceText(range, sm_, lang_opts);
      if (existing == qualified) {
        return;
      }

      const uint32_t loc_key = range.getBegin().getRawEncoding();
      if (!edited_locs_.insert(loc_key).second) {
        return;
      }
      state_.rewriter->ReplaceText(range, qualified);
    }
  }
};

// Start of the source range to replace with a fully qualified type name:
//
// - `S` → name_loc (`S`)
// - `B::S` → begin of `B`
// - `const B::S` → begin of `B` (peels QualifiedTypeLoc first)
static clang::SourceLocation
find_type_name_rewrite_begin(const clang::TypeLoc type_loc,
                             const clang::SourceLocation name_loc,
                             clang::ASTContext &ast) {
  const clang::DynTypedNodeList parents = ast.getParents(type_loc);
  if (parents.empty()) {
    return name_loc;
  }
  const clang::TypeLoc *parent_tl = parents[0].get<clang::TypeLoc>();
  if (!parent_tl) {
    return name_loc;
  }
  const clang::ElaboratedTypeLoc etl =
      parent_tl->getUnqualifiedLoc().getAs<clang::ElaboratedTypeLoc>();
  if (etl.isNull()) {
    return name_loc;
  }
  const clang::NestedNameSpecifierLoc qualifier = etl.getQualifierLoc();
  if (!qualifier) {
    return name_loc;
  }
  return qualifier.getBeginLoc();
}

static bool type_has_declaration_before(const clang::NamedDecl &named_decl,
                                        clang::SourceLocation loc,
                                        const clang::SourceManager &sm) {
  if (!loc.isValid()) {
    return false;
  }
  for (const clang::Decl *redecl : named_decl.redecls()) {
    if (redecl->getFriendObjectKind() == clang::Decl::FOK_Undeclared) {
      continue;
    }
    const clang::SourceLocation redecl_loc = redecl->getLocation();
    if (redecl_loc.isValid() && sm.isBeforeInTranslationUnit(redecl_loc, loc)) {
      return true;
    }
  }
  return false;
}

// Rewrite a type NestedNameSpecifier such as `Instance::StaticData` in
// `Instance::StaticData::get()` to the type's fully qualified name. Needed
// because DeclRefExpr qualifiers are often a single TypeSpec NNS, so there is
// no separate leading `Instance` TagTypeLoc for TypeQualifierInserter.
// Returns true if a rewrite was applied.
static bool try_qualify_type_nested_name_specifier(
    LocalCodeState &state,
    std::unordered_set<uint32_t> &edited_locs,
    clang::NestedNameSpecifierLoc nns_loc) {
  if (!nns_loc) {
    return false;
  }
  clang::TypeLoc type_loc = nns_loc.getTypeLoc();
  if (type_loc.isNull()) {
    return false;
  }
  clang::SourceManager &sm = state.rewriter->getSourceMgr();
  const clang::LangOptions &lang_opts = state.rewriter->getLangOpts();

  clang::SourceLocation begin = nns_loc.getBeginLoc();
  clang::TypeLoc cur = type_loc;
  if (const clang::QualifiedTypeLoc qtl =
          cur.getAs<clang::QualifiedTypeLoc>()) {
    cur = qtl.getUnqualifiedLoc();
  }

  // Written `D<int>::…` uses TemplateSpecializationTypeLoc.
  if (!cur.getAsAdjusted<clang::TemplateSpecializationTypeLoc>().isNull()) {
    return false;
  }

  clang::SourceLocation name_loc;
  const clang::NamedDecl *named_decl = nullptr;
  if (const clang::TagTypeLoc tag = cur.getAsAdjusted<clang::TagTypeLoc>()) {
    if (tag.isDefinition() ||
        llvm::isa<clang::ClassTemplateSpecializationDecl>(tag.getDecl())) {
      return false;
    }
    name_loc = tag.getNameLoc();
    named_decl = tag.getDecl();
  } else if (const clang::TypedefTypeLoc td =
                 cur.getAsAdjusted<clang::TypedefTypeLoc>()) {
    name_loc = td.getNameLoc();
    named_decl = td.getTypedefNameDecl();
  } else {
    return false;
  }

  if (!name_loc.isValid() || !named_decl) {
    return false;
  }
  if (!type_has_declaration_before(*named_decl, name_loc, sm)) {
    return false;
  }
  if (!mapped_location_is_local(state, begin, sm) &&
      !mapped_location_is_local(state, name_loc, sm)) {
    return false;
  }
  if (named_decl->getLocation() == name_loc) {
    return false;
  }
  // File-context leading names (`A` in `A::VALUE`) are handled by
  // TypeQualifierInserter. This path is for TypeSpec NNS that nominate a
  // nested type (`Instance::StaticData` in `Instance::StaticData::get`).
  if (named_decl->getDeclContext()->isFileContext()) {
    return false;
  }
  const std::string name = named_decl->getNameAsString();
  if (name.empty() || !token_spells_name(name_loc, name, sm, lang_opts)) {
    return false;
  }

  // Nested classes are not file-context decls; still OK because we replace the
  // whole written nested name (`Instance::StaticData`) via printQualifiedName.
  const string qualified =
      generate_fully_qualified_name_for_rewrite(*named_decl, lang_opts);
  const clang::CharSourceRange range =
      clang::CharSourceRange::getTokenRange(begin, name_loc);
  if (!range.isValid()) {
    return false;
  }
  const llvm::StringRef existing =
      clang::Lexer::getSourceText(range, sm, lang_opts);
  if (existing == qualified) {
    return false;
  }
  const uint32_t loc_key = range.getBegin().getRawEncoding();
  if (!edited_locs.insert(loc_key).second) {
    return false;
  }
  state.rewriter->ReplaceText(range, qualified);
  return true;
}

// Skip TypeLocs that are not ordinary type-ids. Qualifying these is wrong:
//
// - No prior declaration: `struct B` / `friend class B` can introduce `B`, but
//   a qualified name requires `B` to already exist. If a declaration appears
//   before the use, qualifying is fine (e.g. `friend R`, or `B *` after `struct
//   B`).
// - Nested-name *inner* components: the `C` in `test::C::VALUE` (would become
//   `test::::test::C`). The *leading* component (`Instance` in
//   `Instance::StaticData`) has no prefix and should be qualified so
//   using-directives cannot make it ambiguous.
// - Destructor name: `~C()` / `p->~C()` (must not become `~::C()`).
// - Constructor name: `A::A(...)` / `A(...)` (must not become `A::::ns::A`).
//   This also covers TypeLocs in Clang-synthesized defaulted move-ctor bodies
//   (e.g. implicit static_cast) that reuse the constructor name location.
// - Using-declarator / inheriting ctor: `using Base::Base`.
// - Implicit CXXConstructExpr TypeLocs: copy/move construction can place a
//   TypeLoc for the parameter type at the argument expression (e.g. a
//   parameter named `string` rewritten as `::std::string`). Written type
//   constructions are CXXTemporaryObjectExpr (`T(args)` / `T{args}`).
// - Lambda capture fields: capture FieldDecls reuse the body's use location
//   for their TypeLoc name (same `string` → `::std::string` failure).
static bool
should_skip_type_loc_for_qualifier(clang::TypeLoc type_loc,
                                   const clang::NamedDecl &named_decl,
                                   clang::SourceLocation name_loc,
                                   clang::ASTContext &ast) {
  if (!type_has_declaration_before(
          named_decl, name_loc, ast.getSourceManager())) {
    return true;
  }
  clang::DynTypedNode node = clang::DynTypedNode::create(type_loc);
  while (true) {
    const clang::DynTypedNodeList parents = ast.getParents(node);
    if (parents.empty()) {
      return false;
    }
    const clang::TypeLoc *sugar_parent = nullptr;
    for (const clang::DynTypedNode &parent : parents) {
      if (const auto *nns = parent.get<clang::NestedNameSpecifierLoc>()) {
        if (nns->getPrefix()) {
          return true;
        }
      }
      if (!sugar_parent) {
        sugar_parent = parent.get<clang::TypeLoc>();
      }
    }
    if (sugar_parent) {
      node = clang::DynTypedNode::create(*sugar_parent);
      continue;
    }
    for (const clang::DynTypedNode &parent : parents) {
      if (parent.get<clang::UsingDecl>() ||
          parent.get<clang::CXXDestructorDecl>() ||
          parent.get<clang::CXXPseudoDestructorExpr>()) {
        return true;
      }
      if (const auto *member = parent.get<clang::MemberExpr>()) {
        if (llvm::isa<clang::CXXDestructorDecl>(member->getMemberDecl())) {
          return true;
        }
      }
      if (const auto *ctor = parent.get<clang::CXXConstructorDecl>()) {
        if (ctor->isInheritingConstructor() ||
            ctor->getNameInfo().getLoc() == name_loc) {
          return true;
        }
      }
      if (const auto *construct = parent.get<clang::CXXConstructExpr>()) {
        if (!llvm::isa<clang::CXXTemporaryObjectExpr>(construct)) {
          return true;
        }
      }
      // Lambda captures invent FieldDecls whose TypeLoc name location is the
      // captured variable's use in the body (e.g. `string` → `::std::string`).
      if (const auto *field = parent.get<clang::FieldDecl>()) {
        if (const auto *record =
                llvm::dyn_cast<clang::CXXRecordDecl>(field->getParent())) {
          if (record->isLambda()) {
            return true;
          }
        }
      }
    }
    // Keep walking through non-type parents. Defaulted move constructors can
    // embed TypeLocs under implicit casts/exprs before the CXXConstructorDecl.
    node = parents[0];
  }
}

class TypeQualifierInserter
    : public clang::ast_matchers::MatchFinder::MatchCallback {
  LocalCodeState &state_;
  clang::SourceManager &sm_;
  std::unordered_set<uint32_t> edited_locs_;

public:
  TypeQualifierInserter(LocalCodeState &state)
      : state_(state), sm_(state.rewriter->getSourceMgr()) {}

  void
  run(const clang::ast_matchers::MatchFinder::MatchResult &result) override {
    clang::SourceLocation name_loc;
    const clang::NamedDecl *named_decl = nullptr;
    clang::TypeLoc matched_tl;

    if (const clang::TypeLoc *type_loc =
            result.Nodes.getNodeAs<clang::TypeLoc>("tagTypeLoc")) {
      const clang::TagTypeLoc tag_tl =
          type_loc->getAsAdjusted<clang::TagTypeLoc>();
      if (tag_tl.isNull() || tag_tl.isDefinition()) {
        return;
      }
      if (llvm::isa<clang::ClassTemplateSpecializationDecl>(tag_tl.getDecl())) {
        return;
      }
      name_loc = tag_tl.getNameLoc();
      named_decl = tag_tl.getDecl();
      matched_tl = tag_tl;
    } else if (const clang::TypeLoc *type_loc =
                   result.Nodes.getNodeAs<clang::TypeLoc>(
                       "templateSpecTypeLoc")) {
      const clang::TemplateSpecializationTypeLoc tst =
          type_loc->getAsAdjusted<clang::TemplateSpecializationTypeLoc>();
      if (tst.isNull()) {
        return;
      }
      name_loc = tst.getTemplateNameLoc();
      const clang::TemplateName template_name =
          tst.getTypePtr()->getTemplateName();
      if (const clang::TemplateDecl *td = template_name.getAsTemplateDecl()) {
        if (const auto *ctd = llvm::dyn_cast<clang::ClassTemplateDecl>(td)) {
          named_decl = ctd->getTemplatedDecl();
        } else if (const auto *tatd =
                       llvm::dyn_cast<clang::TypeAliasTemplateDecl>(td)) {
          named_decl = tatd->getTemplatedDecl();
        }
      }
      matched_tl = tst;
    } else if (const clang::TypeLoc *type_loc =
                   result.Nodes.getNodeAs<clang::TypeLoc>(
                       "deducedTemplateSpecTypeLoc")) {
      const clang::DeducedTemplateSpecializationTypeLoc dtst =
          type_loc
              ->getAsAdjusted<clang::DeducedTemplateSpecializationTypeLoc>();
      if (dtst.isNull()) {
        return;
      }
      name_loc = dtst.getTemplateNameLoc();
      const clang::TemplateName template_name =
          dtst.getTypePtr()->getTemplateName();
      if (const clang::TemplateDecl *td = template_name.getAsTemplateDecl()) {
        if (const auto *ctd = llvm::dyn_cast<clang::ClassTemplateDecl>(td)) {
          named_decl = ctd->getTemplatedDecl();
        } else if (const auto *tatd =
                       llvm::dyn_cast<clang::TypeAliasTemplateDecl>(td)) {
          named_decl = tatd->getTemplatedDecl();
        }
      }
      matched_tl = dtst;
    } else if (const clang::TypeLoc *type_loc =
                   result.Nodes.getNodeAs<clang::TypeLoc>("typedefTypeLoc")) {
      const clang::TypedefTypeLoc typedef_tl =
          type_loc->getAsAdjusted<clang::TypedefTypeLoc>();
      if (typedef_tl.isNull()) {
        return;
      }
      name_loc = typedef_tl.getNameLoc();
      named_decl = typedef_tl.getTypedefNameDecl();
      matched_tl = typedef_tl;
    }

    if (!name_loc.isValid() || !named_decl || matched_tl.isNull()) {
      return;
    }
    if (should_skip_type_loc_for_qualifier(
            matched_tl, *named_decl, name_loc, *result.Context)) {
      return;
    }
    if (!mapped_location_is_local(state_, name_loc, sm_)) {
      return;
    }
    // Don't rewrite the name token of the declaration itself (including
    // implicit ctor params whose TypeLoc points at the class name).
    if (named_decl->getLocation() == name_loc) {
      return;
    }
    const std::string name = named_decl->getNameAsString();
    if (name.empty()) {
      return;
    }
    const clang::LangOptions &lang_opts = state_.rewriter->getLangOpts();
    if (!token_spells_name(name_loc, name, sm_, lang_opts)) {
      return;
    }
    const clang::DeclContext *decl_ctx = named_decl->getDeclContext();
    if (!decl_ctx->isFileContext()) {
      return;
    }

    const string qualified =
        generate_fully_qualified_name_for_rewrite(*named_decl, lang_opts);

    const clang::SourceLocation begin =
        find_type_name_rewrite_begin(matched_tl, name_loc, *result.Context);
    const clang::CharSourceRange range =
        clang::CharSourceRange::getTokenRange(begin, name_loc);
    if (!range.isValid()) {
      return;
    }
    const llvm::StringRef existing =
        clang::Lexer::getSourceText(range, sm_, lang_opts);
    if (existing == qualified) {
      return;
    }
    const uint32_t loc_key = range.getBegin().getRawEncoding();
    if (!edited_locs_.insert(loc_key).second) {
      return;
    }
    state_.rewriter->ReplaceText(range, qualified);
  }
};

class UsingNamespaceQualifierInserter
    : public clang::ast_matchers::MatchFinder::MatchCallback {
  LocalCodeState &state_;
  clang::SourceManager &sm_;
  std::unordered_set<uint32_t> edited_locs_;

public:
  UsingNamespaceQualifierInserter(LocalCodeState &state)
      : state_(state), sm_(state.rewriter->getSourceMgr()) {}

  void
  run(const clang::ast_matchers::MatchFinder::MatchResult &result) override {
    const clang::UsingDirectiveDecl *ud =
        result.Nodes.getNodeAs<clang::UsingDirectiveDecl>("usingNamespace");
    if (!ud) {
      return;
    }
    if (!mapped_location_is_local(state_, ud->getLocation(), sm_)) {
      return;
    }
    const clang::NamespaceDecl *nominated = ud->getNominatedNamespace();
    if (!nominated || nominated->isAnonymousNamespace()) {
      return;
    }
    const std::string name = nominated->getNameAsString();
    if (name.empty()) {
      return;
    }
    const clang::LangOptions &lang_opts = state_.rewriter->getLangOpts();
    const clang::SourceLocation name_loc = ud->getIdentLocation();
    if (!name_loc.isValid() ||
        !token_spells_name(name_loc, name, sm_, lang_opts)) {
      return;
    }
    clang::SourceLocation begin = name_loc;
    if (const clang::NestedNameSpecifierLoc qualifier = ud->getQualifierLoc()) {
      begin = qualifier.getBeginLoc();
    }
    const string qualified =
        generate_fully_qualified_name_for_rewrite(*nominated, lang_opts);
    const clang::CharSourceRange range =
        clang::CharSourceRange::getTokenRange(begin, name_loc);
    if (!range.isValid()) {
      return;
    }
    const llvm::StringRef existing =
        clang::Lexer::getSourceText(range, sm_, lang_opts);
    if (existing == qualified) {
      return;
    }
    const uint32_t loc_key = range.getBegin().getRawEncoding();
    if (!edited_locs_.insert(loc_key).second) {
      return;
    }
    state_.rewriter->ReplaceText(range, qualified);
  }
};

class LocalCodeASTConsumer : public clang::ASTConsumer {
private:
  SymbolRenamer renamer_;
  CallQualifierInserter namespace_qualify_inserter_;
  TypeQualifierInserter type_qualify_inserter_;
  UsingNamespaceQualifierInserter using_namespace_qualify_inserter_;
  UsingNamespaceCollector using_namespace_collector_;
  clang::ast_matchers::MatchFinder finder_;
  [[maybe_unused]] LocalCodeState &state_;

public:
  LocalCodeASTConsumer(LocalCodeState &state)
      : renamer_(state), namespace_qualify_inserter_(state),
        type_qualify_inserter_(state), using_namespace_qualify_inserter_(state),
        using_namespace_collector_(state), state_(state) {
    using namespace clang::ast_matchers;
    auto func_matcher = functionDecl();
    auto var_matcher = varDecl();
    auto tag_matcher = tagDecl();
    auto enum_const_matcher = enumConstantDecl();
    auto typedef_matcher = typedefNameDecl();
    finder_.addMatcher(func_matcher.bind("staticDecl"), &renamer_);
    finder_.addMatcher(var_matcher.bind("staticDecl"), &renamer_);
    finder_.addMatcher(tag_matcher.bind("staticDecl"), &renamer_);
    finder_.addMatcher(enum_const_matcher.bind("staticDecl"), &renamer_);
    finder_.addMatcher(typedef_matcher.bind("staticDecl"), &renamer_);
    finder_.addMatcher(declRefExpr(to(func_matcher)).bind("declRef"),
                       &renamer_);
    finder_.addMatcher(unresolvedLookupExpr().bind("unresolvedLookup"),
                       &renamer_);
    finder_.addMatcher(declRefExpr(to(var_matcher)).bind("declRef"), &renamer_);
    finder_.addMatcher(declRefExpr(to(enum_const_matcher)).bind("declRef"),
                       &renamer_);
    finder_.addMatcher(
        typeLoc(loc(qualType(hasDeclaration(tag_matcher)))).bind("tagTypeLoc"),
        &renamer_);
    finder_.addMatcher(
        templateSpecializationTypeLoc().bind("templateSpecTypeLoc"), &renamer_);
    finder_.addMatcher(typeLoc(loc(deducedTemplateSpecializationType()))
                           .bind("deducedTemplateSpecTypeLoc"),
                       &renamer_);
    finder_.addMatcher(typeLoc(loc(qualType(hasDeclaration(typedef_matcher))))
                           .bind("typedefTypeLoc"),
                       &renamer_);
    finder_.addMatcher(usingDirectiveDecl().bind("usingNamespace"),
                       &using_namespace_collector_);
    if (state.rewriter->getLangOpts().CPlusPlus) {
      finder_.addMatcher(declRefExpr().bind("declRef"),
                         &namespace_qualify_inserter_);
      finder_.addMatcher(typeLoc(loc(qualType(hasDeclaration(tag_matcher))))
                             .bind("tagTypeLoc"),
                         &type_qualify_inserter_);
      finder_.addMatcher(
          templateSpecializationTypeLoc().bind("templateSpecTypeLoc"),
          &type_qualify_inserter_);
      finder_.addMatcher(typeLoc(loc(deducedTemplateSpecializationType()))
                             .bind("deducedTemplateSpecTypeLoc"),
                         &type_qualify_inserter_);
      finder_.addMatcher(typeLoc(loc(qualType(hasDeclaration(typedef_matcher))))
                             .bind("typedefTypeLoc"),
                         &type_qualify_inserter_);
      finder_.addMatcher(usingDirectiveDecl().bind("usingNamespace"),
                         &using_namespace_qualify_inserter_);
    }
  }

  void HandleTranslationUnit(clang::ASTContext &context) override {
    finder_.matchAST(context);
  }

  bool shouldSkipFunctionBody(clang::Decl *decl) override {
    // Having the separate true/false branches helps when checking code
    // coverage.
    if (renamer_.location_is_in_local_code(decl->getLocation())) {
      return false;
    }
    return true;
  }
};

class CustomDiagnosticsConsumer : public clang::DiagnosticConsumer {
  const LocalCodeState &state_;
  unique_ptr<clang::DiagnosticConsumer> inner_;

  // Notes after an ignored diagnostic are suppressed until the next non-ignored
  // diagnostic.
  bool ignore_notes_ = false;

public:
  CustomDiagnosticsConsumer(const LocalCodeState &state,
                            unique_ptr<clang::DiagnosticConsumer> inner)
      : state_(state), inner_(std::move(inner)) {}

  void BeginSourceFile(const clang::LangOptions &lang_opts,
                       const clang::Preprocessor *pp) override {
    inner_->BeginSourceFile(lang_opts, pp);
  }

  void EndSourceFile() override { inner_->EndSourceFile(); }

  void finish() override { inner_->finish(); }

  void clear() override {
    DiagnosticConsumer::clear();
    inner_->clear();
    ignore_notes_ = false;
  }

  bool IncludeInDiagnosticCounts() const override {
    return inner_->IncludeInDiagnosticCounts();
  }

  void HandleDiagnostic(clang::DiagnosticsEngine::Level level,
                        const clang::Diagnostic &info) override {
    if (level == clang::DiagnosticsEngine::Note) {
      if (ignore_notes_) {
        return;
      }
    } else {
      ignore_notes_ = this->should_ignore_diagnostic(info);
      if (ignore_notes_) {
        return;
      }
    }
    inner_->HandleDiagnostic(level, info);
  }

private:
  bool should_ignore_diagnostic(const clang::Diagnostic &info) const {
    // Don't ignore any diagnostics for now.
    return false;
  }
};

class RewriteLocalCodeAction : public clang::ASTFrontendAction {
private:
  LocalCodeState &state_;

public:
  RewriteLocalCodeAction(LocalCodeState &state) : state_(state) {}

  unique_ptr<clang::ASTConsumer>
  CreateASTConsumer(clang::CompilerInstance &compiler,
                    const llvm::StringRef file) override {
    compiler.getFrontendOpts().SkipFunctionBodies = true;
    state_.rewriter.emplace(compiler.getSourceManager(),
                            compiler.getLangOpts());
    return std::make_unique<LocalCodeASTConsumer>(state_);
  }
};

// Prefer the longest matching prefix (e.g. BUILD_DIR over REPO_DIR).
// Returns the normalized absolute FROM path and its replacement token.
static optional<PathMap> find_best_path_map(const path &abs_path,
                                            span<const PathMap> path_maps) {
  const string abs_str = abs_path.string();
  optional<PathMap> best;
  size_t best_len = 0;
  for (const PathMap &mapping : path_maps) {
    const path abs_from =
        std::filesystem::absolute(mapping.path).lexically_normal();
    const string from_str = abs_from.string();
    if (!abs_str.starts_with(from_str)) {
      continue;
    }
    if (abs_str.size() > from_str.size() && abs_str[from_str.size()] != '/') {
      continue;
    }
    if (!best || from_str.size() > best_len) {
      best = PathMap{abs_from, mapping.replacement};
      best_len = from_str.size();
    }
  }
  return best;
}

static string apply_path_maps(const path &include,
                              span<const PathMap> path_maps) {
  if (path_maps.empty()) {
    return include.lexically_normal().string();
  }
  const path abs_include =
      include.is_absolute()
          ? include.lexically_normal()
          : std::filesystem::absolute(include).lexically_normal();
  const optional<PathMap> best = find_best_path_map(abs_include, path_maps);
  if (!best) {
    return abs_include.string();
  }
  const string abs_str = abs_include.string();
  const string from_str = best->path.string();
  if (abs_str.size() == from_str.size()) {
    return best->replacement;
  }
  return best->replacement + abs_str.substr(from_str.size());
}

static int handle__extract_local_code(const Cmd_ExtractLocalCode &args) {
  // The driver job args start with "-cc1"; CompilerInvocation expects the
  // remaining cc1 options only (same as clang's cc1_main entry point).
  vector<const char *> cc1_args;
  cc1_args.reserve(args.clang_args.size());
  for (const string &arg : args.clang_args) {
    if (cc1_args.empty() && arg == "-cc1") {
      continue;
    }
    cc1_args.push_back(arg.c_str());
  }

  llvm::InitializeAllTargets();
  llvm::InitializeAllTargetMCs();
  llvm::InitializeAllAsmPrinters();
  llvm::InitializeAllAsmParsers();

  Config config = Config::from_paths(args.config_paths);
  LocalCodeState state{args, config};

  // Handle the preprocessing part.
  {
    clang::CompilerInstance clang_instance;
    clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
        new clang::DiagnosticIDs());
    clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_opts(
        new clang::DiagnosticOptions());
    clang::TextDiagnosticBuffer *diags_buffer =
        new clang::TextDiagnosticBuffer();
    clang::DiagnosticsEngine diags(diag_id, diag_opts, diags_buffer);

    bool success = clang::CompilerInvocation::CreateFromArgs(
        clang_instance.getInvocation(), cc1_args, diags);
    clang_instance.createDiagnostics();
    diags_buffer->FlushDiagnostics(clang_instance.getDiagnostics());
    if (!success) {
      return 1;
    }

    state.object_path = clang_instance.getFrontendOpts().OutputFile;

    const auto &inputs = clang_instance.getFrontendOpts().Inputs;
    if (!inputs.empty()) {
      state.input_kind = inputs.front().getKind();
    }

    ExtractPreprocessedLocalCodeAction action(state);
    success = clang_instance.ExecuteAction(action);
    if (!success) {
      return 1;
    }
  }

  // Do semantic changes.
  {
    clang::CompilerInstance clang_instance;
    clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
        new clang::DiagnosticIDs());
    clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_opts(
        new clang::DiagnosticOptions());
    clang::TextDiagnosticBuffer *diags_buffer =
        new clang::TextDiagnosticBuffer();
    clang::DiagnosticsEngine diags(diag_id, diag_opts, diags_buffer);

    bool success = clang::CompilerInvocation::CreateFromArgs(
        clang_instance.getInvocation(), cc1_args, diags);
    clang_instance.createDiagnostics();
    diags_buffer->FlushDiagnostics(clang_instance.getDiagnostics());
    if (!success) {
      return 1;
    }
    clang_instance.getDiagnostics().setClient(
        new CustomDiagnosticsConsumer(
            state, clang_instance.getDiagnostics().takeClient()),
        true);
    auto &inputs = clang_instance.getFrontendOpts().Inputs;
    clang::InputKind kind = inputs.front().getKind().getPreprocessed();
    inputs.clear();
    inputs.emplace_back(
        llvm::MemoryBufferRef(state.code_for_parser, "preprocessed_code"),
        kind);

    RewriteLocalCodeAction action(state);
    success = clang_instance.ExecuteAction(action);
    if (!success) {
      return 1;
    }

    clang::RewriteBuffer &buffer = state.rewriter->getEditBuffer(
        state.rewriter->getSourceMgr().getMainFileID());
    llvm::raw_string_ostream os(state.rewrite_result);
    buffer.write(os);
    os.flush();

    vector<string_view> lines = split_into_lines(state.rewrite_result);
    assert(lines.size() == state.map_parser_to_local_lines.size());
    for (size_t i = 0; i < lines.size(); i++) {
      const int mapped_line = state.map_parser_to_local_lines[i];
      if (mapped_line == -1) {
        continue;
      }
      state.local_code_lines[mapped_line] = lines[i];
    }
  }

  // Write the local code output as frontmatter (.toml) + preprocessed code
  // (.i / .ii).
  {
    const bool is_cxx = state.input_kind.getLanguage() == clang::Language::CXX;
    path code_path = args.local_code_path;
    code_path.replace_extension(is_cxx ? "ii" : "i");

    {
      error_code ec;
      llvm::raw_fd_ostream fs(code_path.string(), ec);
      if (ec) {
        fmt::println(stderr,
                     "Could not write local code file: {}: {}",
                     code_path.string(),
                     ec.message());
        return 1;
      }
      for (const string_view line : state.local_code_lines) {
        fs << line << '\n';
      }
    }

    LocalCodeInfo info;
    info.local_code_path = code_path.filename();
    info.source_language =
        string(clang::languageToString(state.input_kind.getLanguage()));
    info.include_defines = state.include_defines;
    info.direct_includes.reserve(state.direct_includes.size());
    for (const LocalCodeInfo::DirectInclude &include : state.direct_includes) {
      info.direct_includes.push_back(LocalCodeInfo::DirectInclude{
          .include_path =
              path(apply_path_maps(include.include_path, args.path_maps)),
          .is_system_header = include.is_system_header,
          .is_pure_c = include.is_pure_c,
      });
    }
    const bool write_all = args.path_maps.empty();
    if (write_all) {
      // Can't properly apply path maps here, so skip it. Otherwise local-code
      // tests are not easily reproducible.
      vector<string> cc1_arg_strings;
      cc1_arg_strings.reserve(cc1_args.size());
      for (const char *arg : cc1_args) {
        cc1_arg_strings.emplace_back(arg);
      }
      info.cc1_args = std::move(cc1_arg_strings);
      info.cwd = std::filesystem::current_path();
      info.object_path = state.object_path;
    }
    info.using_namespaces = std::move(state.using_namespaces);
    if (!info.write_to_path(args.local_code_path)) {
      fmt::println(stderr,
                   "Could not write local code info: {}",
                   args.local_code_path.string());
      return 1;
    }
  }

  return 0;
}

struct CompileLocalCodeState {
  std::unordered_map<path, size_t> seen_direct_include_paths;
  vector<LocalCodeInfo::DirectInclude> ordered_direct_include_paths;
  vector<string> all_include_defines;
  string merged_local_code;
  string language;

  string preprocessed_headers;
  string combined_preprocessed_code;
};

static vector<path>
get_header_search_paths(const clang::HeaderSearchOptions &opts) {
  vector<path> prefixes;
  prefixes.reserve(opts.UserEntries.size() + 1);
  for (const clang::HeaderSearchOptions::Entry &entry : opts.UserEntries) {
    prefixes.push_back(path(entry.Path).lexically_normal());
  }
  if (!opts.ResourceDir.empty()) {
    prefixes.push_back((path(opts.ResourceDir) / "include").lexically_normal());
  }
  return prefixes;
}

// Try to find the best include path based on the search prefixes. This is
// important so that #include_next works properly (which doesn't work when
// including everything with absolute paths).
static string
get_best_include_path_spelling(const path &include_path,
                               const vector<path> &search_prefixes) {
  optional<string> best;
  size_t best_prefix_native_size = 0;
  for (const path &prefix : search_prefixes) {
    const path relative = include_path.lexically_relative(prefix);
    if (relative.empty() || relative == ".") {
      continue;
    }
    const auto first = relative.begin();
    if (first != relative.end() && *first == "..") {
      continue;
    }
    const size_t prefix_native_size = prefix.native().size();
    if (!best || prefix_native_size > best_prefix_native_size) {
      best_prefix_native_size = prefix_native_size;
      best = relative.generic_string();
    }
  }
  return best.value_or(include_path.string());
}

static void add_direct_include(CompileLocalCodeState &state,
                               path include_path,
                               bool is_system_header,
                               bool is_pure_c) {
  const auto [it, inserted] = state.seen_direct_include_paths.emplace(
      include_path, state.ordered_direct_include_paths.size());
  if (inserted) {
    state.ordered_direct_include_paths.push_back(LocalCodeInfo::DirectInclude{
        .include_path = std::move(include_path),
        .is_system_header = is_system_header,
        .is_pure_c = is_pure_c,
    });
    return;
  }
  state.ordered_direct_include_paths[it->second].is_system_header |=
      is_system_header;
  state.ordered_direct_include_paths[it->second].is_pure_c |= is_pure_c;
}

class PreprocessHeadersAction : public clang::PreprocessorFrontendAction {
private:
  CompileLocalCodeState &state_;

public:
  PreprocessHeadersAction(CompileLocalCodeState &state) : state_(state) {}

  void ExecuteAction() override {
    clang::CompilerInstance &compiler = this->getCompilerInstance();
    clang::Preprocessor &pp = compiler.getPreprocessor();
    llvm::raw_string_ostream os(state_.preprocessed_headers);
    clang::PreprocessorOutputOptions opts;
    opts.ShowCPP = true;
    opts.ShowLineMarkers = true;
    clang::DoPrintPreprocessedInput(pp, &os, opts);
    os.flush();
  }
};

static int handle__compile_local_code(const Cmd_CompileLocalCode &args) {
  // The driver job args start with "-cc1"; CompilerInvocation expects the
  // remaining cc1 options only (same as clang's cc1_main entry point).
  vector<const char *> cc1_args;
  cc1_args.reserve(args.clang_args.size());
  for (const string &arg : args.clang_args) {
    if (cc1_args.empty() && arg == "-cc1") {
      continue;
    }
    cc1_args.push_back(arg.c_str());
  }

  llvm::InitializeAllTargets();
  llvm::InitializeAllTargetMCs();
  llvm::InitializeAllAsmPrinters();
  llvm::InitializeAllAsmParsers();

  CompileLocalCodeState state;

  // Gather inputs from frontmatter (.toml) + preprocessed code (.i / .ii).
  for (const path &frontmatter_path : args.local_code_paths) {
    optional<LocalCodeInfo> frontmatter_opt =
        LocalCodeInfo::from_path(frontmatter_path);
    if (!frontmatter_opt) {
      fmt::println(stderr,
                   "Could not read local code frontmatter: {}",
                   frontmatter_path.string());
      return 1;
    }
    const LocalCodeInfo &frontmatter = *frontmatter_opt;

    path code_path = frontmatter.local_code_path;
    if (code_path.empty()) {
      fmt::println(stderr,
                   "Missing local_code_path in frontmatter: {}",
                   frontmatter_path.string());
      return 1;
    }
    if (code_path.is_relative()) {
      code_path = frontmatter_path.parent_path() / code_path;
    }

    std::ifstream code_fs(code_path);
    if (!code_fs.is_open()) {
      fmt::println(
          stderr, "Could not open local code file: {}", code_path.string());
      return 1;
    }
    string code_str((std::istreambuf_iterator<char>(code_fs)),
                    std::istreambuf_iterator<char>());

    for (const LocalCodeInfo::DirectInclude &include :
         frontmatter.direct_includes) {
      add_direct_include(state,
                         include.include_path,
                         include.is_system_header,
                         include.is_pure_c);
    }
    for (const string &define : frontmatter.include_defines) {
      state.all_include_defines.push_back(define);
    }
    state.language = frontmatter.source_language;
    // Isolate diagnostics per file.
    state.merged_local_code.append("#pragma GCC diagnostic push\n");
    state.merged_local_code.append("#pragma clang diagnostic push\n");
    state.merged_local_code.append(code_str);
    state.merged_local_code.append("#pragma clang diagnostic pop\n");
    state.merged_local_code.append("#pragma GCC diagnostic pop\n");
  }

  // Preprocess headers.
  {
    clang::CompilerInstance clang_instance;
    clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
        new clang::DiagnosticIDs());
    clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_opts(
        new clang::DiagnosticOptions());
    clang::TextDiagnosticBuffer *diags_buffer =
        new clang::TextDiagnosticBuffer();
    clang::DiagnosticsEngine diags(diag_id, diag_opts, diags_buffer);

    bool success = clang::CompilerInvocation::CreateFromArgs(
        clang_instance.getInvocation(), cc1_args, diags);
    clang_instance.createDiagnostics();
    diags_buffer->FlushDiagnostics(clang_instance.getDiagnostics());
    if (!success) {
      return 1;
    }

    const vector<path> search_prefixes =
        get_header_search_paths(clang_instance.getHeaderSearchOpts());

    string code_to_preprocess;
    for (const string_view define : state.all_include_defines) {
      code_to_preprocess.append(define);
      code_to_preprocess += '\n';
    }

    // Handle system includes in a special way because they have other rules for
    // warnings etc. Use search-path-relative spellings so #include_next in
    // resource headers (e.g. limits.h) still works.
    vector<string> system_wrapper_bodies;
    system_wrapper_bodies.reserve(state.ordered_direct_include_paths.size());
    size_t system_wrapper_i = 0;
    const bool wrap_pure_c = state.language == "C++";
    for (const LocalCodeInfo::DirectInclude &include :
         state.ordered_direct_include_paths) {
      const string spelling =
          get_best_include_path_spelling(include.include_path, search_prefixes);
      const bool needs_extern_c = wrap_pure_c && include.is_pure_c;
      if (needs_extern_c) {
        code_to_preprocess.append("extern \"C\" {\n");
      }
      if (include.is_system_header) {
        const string wrapper_path = fmt::format(
            "/__ccelerate__/system_include_{}.h", system_wrapper_i++);
        system_wrapper_bodies.push_back(fmt::format(
            "#pragma GCC system_header\n#include <{}>\n", spelling));
        code_to_preprocess.append("#include <");
        code_to_preprocess.append(wrapper_path);
        code_to_preprocess.append(">\n");
      } else {
        code_to_preprocess.append("#include <");
        code_to_preprocess.append(spelling);
        code_to_preprocess.append(">\n");
      }
      if (needs_extern_c) {
        code_to_preprocess.append("}\n");
      }
    }

    // Add wrappers for system includes.
    llvm::IntrusiveRefCntPtr<llvm::vfs::OverlayFileSystem> overlay =
        llvm::makeIntrusiveRefCnt<llvm::vfs::OverlayFileSystem>(
            llvm::vfs::getRealFileSystem());
    llvm::IntrusiveRefCntPtr<llvm::vfs::InMemoryFileSystem> memfs =
        llvm::makeIntrusiveRefCnt<llvm::vfs::InMemoryFileSystem>();
    for (size_t i = 0; i < system_wrapper_bodies.size(); i++) {
      const string wrapper_path =
          fmt::format("/__ccelerate__/system_include_{}.h", i);
      memfs->addFile(wrapper_path,
                     0,
                     llvm::MemoryBuffer::getMemBufferCopy(
                         system_wrapper_bodies[i], wrapper_path));
    }
    overlay->pushOverlay(memfs);
    clang_instance.createFileManager(overlay);

    auto &inputs = clang_instance.getFrontendOpts().Inputs;
    clang::InputKind kind = clang::InputKind(
        state.language == "C++" ? clang::Language::CXX : clang::Language::C);
    inputs.clear();
    inputs.emplace_back(
        llvm::MemoryBufferRef(code_to_preprocess, "core_to_preprocess"), kind);

    PreprocessHeadersAction action(state);
    success = clang_instance.ExecuteAction(action);
    if (!success) {
      return 1;
    }
  }
  {
    state.combined_preprocessed_code = state.preprocessed_headers;
    state.combined_preprocessed_code += "\n\n";
    state.combined_preprocessed_code += state.merged_local_code;

    clang::CompilerInstance clang_instance;
    clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
        new clang::DiagnosticIDs());
    clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_opts(
        new clang::DiagnosticOptions());
    clang::TextDiagnosticBuffer *diags_buffer =
        new clang::TextDiagnosticBuffer();
    clang::DiagnosticsEngine diags(diag_id, diag_opts, diags_buffer);

    bool success = clang::CompilerInvocation::CreateFromArgs(
        clang_instance.getInvocation(), cc1_args, diags);
    clang_instance.createDiagnostics();
    diags_buffer->FlushDiagnostics(clang_instance.getDiagnostics());
    if (!success) {
      return 1;
    }
    auto &inputs = clang_instance.getFrontendOpts().Inputs;
    clang::InputKind kind =
        clang::InputKind(state.language == "C++" ? clang::Language::CXX
                                                 : clang::Language::C)
            .getPreprocessed();
    inputs.clear();
    inputs.emplace_back(llvm::MemoryBufferRef(state.combined_preprocessed_code,
                                              "combined_preprocessed_code"),
                        kind);

    clang_instance.getFrontendOpts().OutputFile = args.obj_output_path.string();

    clang::EmitObjAction action;
    success = clang_instance.ExecuteAction(action);
    if (!success) {
      return 1;
    }
  }

  return 0;
}

int clang_ops_main(const int argc, char **argv) {
  CLI::App app{"clang_for_ccelerate"};
  app.description("A utility used by ccelerate to do clang specific things "
                  "like argument parsing");

  CLI::App &parse_args_cmd = *app.add_subcommand("parse-args");
  Cmd_ParseArgs parse_args;
  parse_args_cmd.add_option("--cwd", parse_args.cwd)->required();
  parse_args_cmd.add_option("--binary", parse_args.binary)->required();
  parse_args_cmd.add_option(
      "passthrough", parse_args.clang_args, "Arguments passed to clang");

  CLI::App &compile_obj_cmd = *app.add_subcommand("compile-object");
  Cmd_CompileObj compile_obj_args;
  compile_obj_cmd.add_option(
      "passthrough", compile_obj_args.clang_args, "Arguments passed to clang");

  CLI::App &extract_local_code_cmd = *app.add_subcommand("local-code");
  Cmd_ExtractLocalCode extract_local_code_args;
  extract_local_code_cmd.add_option("--local-code-path",
                                    extract_local_code_args.local_code_path,
                                    "Path to write the local code frontmatter "
                                    "(.toml); preprocessed code is written "
                                    "alongside as .i/.ii");
  extract_local_code_cmd.add_option("--local-id",
                                    extract_local_code_args.local_id,
                                    "Suffix to use to make symbols unique");
  vector<string> path_map_args;
  extract_local_code_cmd
      .add_option("--path-map",
                  path_map_args,
                  "Map FROM=TO for paths in frontmatter (repeatable)")
      ->allow_extra_args(false);
  extract_local_code_cmd
      .add_option("--config",
                  extract_local_code_args.config_paths,
                  "Path to a ccelerate config .toml (repeatable)")
      ->allow_extra_args(false);
  extract_local_code_cmd.add_option("passthrough",
                                    extract_local_code_args.clang_args,
                                    "Arguments passed to clang");

  CLI::App &compile_local_code_cmd = *app.add_subcommand("compile-local-code");
  Cmd_CompileLocalCode compile_local_code_args;
  compile_local_code_cmd.add_option("--input",
                                    compile_local_code_args.local_code_paths,
                                    "Path to local code frontmatter .toml "
                                    "(repeatable)");
  compile_local_code_cmd
      .add_option("--output",
                  compile_local_code_args.obj_output_path,
                  "Path to write the object file to")
      ->required();
  compile_local_code_cmd.add_option("passthrough",
                                    compile_local_code_args.clang_args,
                                    "Arguments passed to clang");

  CLI11_PARSE(app, argc, argv);

  if (parse_args_cmd) {
    return handle__parse_args(parse_args);
  } else if (compile_obj_cmd) {
    return handle__compile_obj(compile_obj_args);
  } else if (extract_local_code_cmd) {
    for (const string &map_arg : path_map_args) {
      const size_t eq = map_arg.find('=');
      if (eq == string::npos || eq == 0) {
        fmt::println(
            stderr, "Invalid --path-map '{}', expected FROM=TO", map_arg);
        return 1;
      }
      extract_local_code_args.path_maps.push_back(
          PathMap{path(map_arg.substr(0, eq)), map_arg.substr(eq + 1)});
    }
    return handle__extract_local_code(extract_local_code_args);
  } else if (compile_local_code_cmd) {
    return handle__compile_local_code(compile_local_code_args);
  } else {
    fmt::println(stderr, "Unknown command: {}", parse_args_cmd.get_name());
  }

  return 1;
}

} // namespace ccelerate

int main(int argc, char **argv) {
  return ccelerate::clang_ops_main(argc, argv);
}