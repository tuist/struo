#include <string.h>
#include <stdlib.h>
int alpha(void);
int beta(void);
int main(int argc, char **argv) {
    if (getenv("STRUO_FIXTURE_CHILD") && system("true") != 0) return 4;
    if (getenv("STRUO_FIXTURE_FAIL")) return 1;
    if (argc != 2) return 2;
    if (strcmp(argv[1], "alpha") == 0) return alpha() == 17 ? 0 : 1;
    if (strcmp(argv[1], "beta") == 0) return beta() == 31 ? 0 : 1;
    return 3;
}
