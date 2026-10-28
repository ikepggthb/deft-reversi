// Generate Egaroucid's own fallback hash data once, with a fixed seed.
// Build with -I /path/to/Egaroucid/src and write outside the source checkout.
#include "engine/bit.hpp"
#include "engine/hash.hpp"
#include <cstdio>
#include <cstdlib>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    raw_myrandom.seed(0x20261027u);
    hash_init_rand(23);
    FILE *out = std::fopen(argv[1], "wb");
    if (!out) return 3;
    for (int i = 0; i < 4; ++i)
        if (std::fwrite(hash_rand_player[i], sizeof(uint32_t), 65536, out) != 65536) return 4;
    for (int i = 0; i < 4; ++i)
        if (std::fwrite(hash_rand_opponent[i], sizeof(uint32_t), 65536, out) != 65536) return 4;
    return std::fclose(out) ? 5 : 0;
}
