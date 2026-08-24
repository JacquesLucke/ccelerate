```sh
git submodule update --init --recursive
cmake --workflow --preset release
# Disable more expensive tracing: TRACY_NO_SYS_TRACE=1
```