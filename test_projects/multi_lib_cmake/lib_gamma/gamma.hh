// SPDX-License-Identifier: MIT

#pragma once

#include <string>

namespace multi_lib::gamma {

std::string part_1();
std::string part_2();
std::string part_3();
std::string part_4();
std::string part_5();

inline std::string combine() {
  return part_1() + part_2() + part_3() + part_4() + part_5();
}

} // namespace multi_lib::gamma
