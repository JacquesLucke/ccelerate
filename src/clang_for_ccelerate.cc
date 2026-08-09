// SPDX-License-Identifier: MIT

#include <CLI/CLI.hpp>
#include <algorithm>
#include <clang/AST/ASTConsumer.h>
#include <clang/AST/DeclCXX.h>
#include <clang/AST/TypeLoc.h>
#include <clang/ASTMatchers/ASTMatchFinder.h>
#include <clang/ASTMatchers/ASTMatchers.h>
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
#include <clang/Rewrite/Core/Rewriter.h>
#include <clang/Tooling/CommonOptionsParser.h>
#include <clang/Tooling/Tooling.h>
#include <ctre.hpp>
#include <filesystem>
#include <fmt/format.h>
#include <llvm/Config/llvm-config.h>
#include <llvm/Support/CommandLine.h>
#include <llvm/Support/TargetSelect.h>
#include <llvm/Support/VirtualFileSystem.h>
#include <llvm/TargetParser/Host.h>
#include <toml.hpp>

#include "clang_for_ccelerate_io.hh"
#include "config.hh"
#include "error_code.hh"
#include "get_current_executable_path.hh"
#include "memory.hh"
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
  optional<path> frontmatter_base;
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

  clang::InputKind input_kind;
  string raw_preprocessed;
  string code_for_parser;
  vector<string_view> local_code_lines;
  vector<int> map_parser_to_local_lines;
  std::unordered_set<path> direct_includes;

  optional<clang::Rewriter> rewriter;
  string rewrite_result;
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
                state_.direct_includes.insert(file);
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
    const std::string old_name = named_decl->getNameAsString();
    if (old_name.empty()) {
      return;
    }
    if (!this->name_should_be_localized(*named_decl)) {
      return;
    }
    const uint32_t loc_key = loc.getRawEncoding();
    if (!renamed_locs_.insert(loc_key).second) {
      return;
    }
    state_.rewriter->InsertTextAfter(loc.getLocWithOffset(old_name.size()),
                                     state_.args.local_id);
  }

private:
  bool name_should_be_localized(const clang::NamedDecl &decl) {
    if (decl.isExternallyVisible()) {
      return false;
    }
    if (const auto *ctor = llvm::dyn_cast<clang::CXXConstructorDecl>(&decl)) {
      const clang::CXXRecordDecl *record = ctor->getParent();
      if (const clang::TagDecl *def = record->getDefinition()) {
        return this->name_should_be_localized(*def);
      }
      return false;
    }
    if (const auto *fd = llvm::dyn_cast<clang::FunctionDecl>(&decl)) {
      if (const clang::FunctionDecl *def = fd->getDefinition()) {
        const clang::DeclContext *dc = def->getDeclContext();
        if (dc->isFunctionOrMethod() || dc->isRecord()) {
          return false;
        }
        return this->location_is_in_local_code(def->getLocation());
      }
    }
    if (const auto *vd = llvm::dyn_cast<clang::VarDecl>(&decl)) {
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
        return this->location_is_in_local_code(def->getLocation());
      }
    }
    if (const auto *ecd = llvm::dyn_cast<clang::EnumConstantDecl>(&decl)) {
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
        return true;
      }
    }
    return false;
  }

  bool location_is_in_local_code(clang::SourceLocation loc) const {

    if (!loc.isValid()) {
      return false;
    }
    const size_t line = sm_.getExpansionLineNumber(loc);
    if (line == 0 || line > state_.map_parser_to_local_lines.size()) {
      return false;
    }
    const int mapped_line = state_.map_parser_to_local_lines[line - 1];
    const bool is_local = mapped_line != -1;
    return is_local;
  }
};

class LocalCodeASTConsumer : public clang::ASTConsumer {
private:
  SymbolRenamer renamer_;
  clang::ast_matchers::MatchFinder finder_;
  [[maybe_unused]] LocalCodeState &state_;

public:
  LocalCodeASTConsumer(LocalCodeState &state) : renamer_(state), state_(state) {
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
    finder_.addMatcher(declRefExpr(to(var_matcher)).bind("declRef"), &renamer_);
    finder_.addMatcher(declRefExpr(to(enum_const_matcher)).bind("declRef"),
                       &renamer_);
    finder_.addMatcher(
        typeLoc(loc(qualType(hasDeclaration(tag_matcher)))).bind("tagTypeLoc"),
        &renamer_);
    finder_.addMatcher(typeLoc(loc(qualType(hasDeclaration(typedef_matcher))))
                           .bind("typedefTypeLoc"),
                       &renamer_);
  }

  void HandleTranslationUnit(clang::ASTContext &context) override {
    finder_.matchAST(context);
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
    state_.rewriter.emplace(compiler.getSourceManager(),
                            compiler.getLangOpts());
    return std::make_unique<LocalCodeASTConsumer>(state_);
  }
};

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

  // Write the local code output.
  {
    error_code ec;
    llvm::raw_fd_ostream fs(state.args.local_code_path.string(), ec);

    // Write frontmatter.
    fs << "+++\n";
    {
      const auto path_for_frontmatter = [&](const path &include) -> string {
        if (!args.frontmatter_base) {
          return include.lexically_normal().string();
        }
        const path abs_include =
            include.is_absolute()
                ? include.lexically_normal()
                : std::filesystem::absolute(include).lexically_normal();
        const path abs_base = std::filesystem::absolute(*args.frontmatter_base)
                                  .lexically_normal();
        const path relative = abs_include.lexically_relative(abs_base);
        if (relative.empty() || *relative.begin() == "..") {
          return abs_include.string();
        }
        return relative.string();
      };

      vector<string> direct_includes;
      direct_includes.reserve(state.direct_includes.size());
      for (const path &include : state.direct_includes) {
        direct_includes.push_back(path_for_frontmatter(include));
      }
      std::sort(direct_includes.begin(), direct_includes.end());
      toml::array_format_info fmt;
      fmt.fmt = toml::array_format::multiline;
      toml::value table;
      table["direct_includes"] = toml::value(direct_includes, fmt);
      table["source_language"] =
          clang::languageToString(state.input_kind.getLanguage());
      string toml_str = toml::format(table);
      fs << trim_whitespace(toml_str) << '\n';
    }
    fs << "+++\n";

    // Actual code.
    for (const string_view line : state.local_code_lines) {
      fs << line << '\n';
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

  CLI::App &compile_obj_cmd = *app.add_subcommand("compile_obj");
  Cmd_CompileObj compile_obj_args;
  compile_obj_cmd.add_option(
      "passthrough", compile_obj_args.clang_args, "Arguments passed to clang");

  CLI::App &extract_local_code_cmd = *app.add_subcommand("local-code");
  Cmd_ExtractLocalCode extract_local_code_args;
  extract_local_code_cmd.add_option("--local-code-path",
                                    extract_local_code_args.local_code_path,
                                    "Path to write the local code to");
  extract_local_code_cmd.add_option("--local-id",
                                    extract_local_code_args.local_id,
                                    "Suffix to use to make symbols unique");
  extract_local_code_cmd.add_option(
      "--frontmatter-base",
      extract_local_code_args.frontmatter_base,
      "If set, paths in frontmatter are made relative to this directory");
  // Vector options default to allow_extra_args, which makes --config eat the
  // "--" separator and leave -cc1 as an unknown option.
  extract_local_code_cmd
      .add_option("--config",
                  extract_local_code_args.config_paths,
                  "Path to a ccelerate config .toml (repeatable)")
      ->allow_extra_args(false);
  extract_local_code_cmd.add_option("passthrough",
                                    extract_local_code_args.clang_args,
                                    "Arguments passed to clang");

  CLI11_PARSE(app, argc, argv);

  if (parse_args_cmd) {
    return handle__parse_args(parse_args);
  } else if (compile_obj_cmd) {
    return handle__compile_obj(compile_obj_args);
  } else if (extract_local_code_cmd) {
    return handle__extract_local_code(extract_local_code_args);
  } else {
    fmt::println(stderr, "Unknown command: {}", parse_args_cmd.get_name());
  }

  return 1;
}

} // namespace ccelerate

int main(int argc, char **argv) {
  return ccelerate::clang_ops_main(argc, argv);
}