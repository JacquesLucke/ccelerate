// SPDX-License-Identifier: MIT

#pragma once

#if defined(_WIN32)
#  if defined(MULTI_LIB_SHARED_BUILD)
#    define MULTI_LIB_API __declspec(dllexport)
#  else
#    define MULTI_LIB_API __declspec(dllimport)
#  endif
#else
#  define MULTI_LIB_API __attribute__((visibility("default")))
#endif
