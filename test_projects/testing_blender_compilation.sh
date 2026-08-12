# Run in /home/jacques/blender/build/ccelerate.

## 2026-08-11
```
/home/jacques/Documents/ccelerate/build/release/bin/ccelerate_cmake /home/jacques/blender/blender -GNinja -DWITH_COMPILER_PRECOMPILED_HEADERS=OFF -DWITH_UNITY_BUILD=OFF -DWITH_GTESTS=ON -DCMAKE_EXPORT_COMPILE_COMMANDS=ON -DWITH_CYCLES_NATIVE_ONLY=ON -DWITH_LINKER_MOLD=ON -DCMAKE_BUILD_TYPE=Release

time ninja install
real	35m22.102s
user	0m36.780s
sys	    0m53.267s

time ninja bf_nodes_geometry
real	8m13.056s
user	0m19.713s
sys	    0m23.452s
```