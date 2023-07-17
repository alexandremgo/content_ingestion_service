## Requirements

LibTorch needs to be installed on your machine/container: `rust-bert`, through `tch-rc`, is bound 
to the C++ Libtorch API.

Check the version of `rust-bert` and `tch-rc` to know which version of LibTorch is needed.
Currently `v2.0.0`.

Then export the following env variables (linux/macOS):

```bash
export LIBTORCH=<path-to-libtorch>
export LD_LIBRARY_PATH=${LIBTORCH}/lib:$LD_LIBRARY_PATH
```
