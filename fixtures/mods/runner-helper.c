/* Synthetic runner witness only; built privately by isolated macOS tests. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2) return 64;
    if (!strcmp(argv[1], "echo")) {
        unsigned char buffer[4096];
        size_t n;
        while ((n = fread(buffer, 1, sizeof buffer, stdin)))
            if (fwrite(buffer, 1, n, stdout) != n) return 74;
        return ferror(stdin) ? 74 : 0;
    }
    if (!strcmp(argv[1], "hang")) {
        for (;;) pause();
    }
    if (!strcmp(argv[1], "flood")) {
        unsigned char buffer[4096];
        memset(buffer, 'X', sizeof buffer);
        for (;;) {
            if (write(STDOUT_FILENO, buffer, sizeof buffer) < 0) return 74;
            if (write(STDERR_FILENO, buffer, sizeof buffer) < 0) return 74;
        }
    }
    return 64;
}
