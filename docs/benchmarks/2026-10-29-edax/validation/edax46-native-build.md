# Edax 4.6 native build record

- Source version: `VERSION_STRING "4.6"` at commit
  `713a434f13b3d15fb69c61b9ba3641aed82c496c`.
- The source was exported with `git archive` to
  `/tmp/edax-4.6-native-20261029/source.tar` (SHA-256
  `5b8718d90a8ae6eb38451b667f453bcdc1be5da7e31ee63943070b51e68b214f`) and
  built from the extracted copy. The checkout itself was not modified.
- Build command: `make CC=clang BUILD=optimize build ARCH=native`.
- Compiler: `clang version 23.1.4`.
- Make completed with exit status 0. The compiler invocation emitted by make
  was:

  ```text
  clang -std=c17 -pedantic -W -Wall -Wno-invalid-source-encoding -D_GNU_SOURCE=1 -O3 -flto -ffast-math -fomit-frame-pointer -DNDEBUG -march=native all.c -o ../bin/lEdax-native -lm -lrt
  ```

- Binary: `/tmp/edax-4.6-native-20261029/source/bin/lEdax-native`, SHA-256
  `406cdfa8aa6e28962d77834e926f72f6a6dec78741f82d1552287761a1d2f0be`.
- Read-only disassembly of that binary contains AVX2 YMM integer instructions,
  including `vpbroadcastq ymm1`, `vpand ymm4`, `vpsrlvq ymm5`, `vpor ymm5`,
  `vpermq ymm0`, and `vpxor ymm7`. This confirms the `-march=native` build
  produced the intended native vector code.
