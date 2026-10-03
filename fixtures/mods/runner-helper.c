/* Synthetic runner witness only; built privately by isolated macOS tests. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <spawn.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <arpa/inet.h>
extern char **environ;

static void result(const char *name, int fd) {
    printf("%s=%s\n", name, fd < 0 ? "denied" : "allowed");
    if (fd >= 0) close(fd);
}
static int probe(int argc, char **argv) {
    if (argc != 8) return 64;
    result("protected_read", open(argv[2], O_RDONLY));
    result("protected_write", open(argv[2], O_WRONLY | O_APPEND));
    result("other_run_read", open(argv[3], O_RDONLY));
    result("other_run_write", open(argv[3], O_WRONLY | O_APPEND));
    /* Both controls start0555; OS policy must deny changing mode, too. */
    int changed = chmod(argv[0], 0755);
    result("program_write", changed == 0 ? open(argv[0], O_WRONLY | O_APPEND) : -1);
    result("scratch_write", open(argv[4], O_WRONLY | O_CREAT, 0600));
    result("symlink_read", open(argv[5], O_RDONLY));
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in tcp = {0};
    tcp.sin_family = AF_INET;
    tcp.sin_port = htons((unsigned short)atoi(argv[6]));
    tcp.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    int connected = fd >= 0 && connect(fd, (struct sockaddr *)&tcp, sizeof tcp) == 0;
    printf("tcp=%s\n", connected ? "allowed" : "denied");
    if (fd >= 0) close(fd);
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un unix_addr = {0};
    unix_addr.sun_family = AF_UNIX;
    if (strlen(argv[7]) >= sizeof unix_addr.sun_path) return 64;
    strcpy(unix_addr.sun_path, argv[7]);
    connected = fd >= 0 && connect(fd, (struct sockaddr *)&unix_addr, sizeof unix_addr) == 0;
    printf("unix=%s\n", connected ? "allowed" : "denied");
    if (fd >= 0) close(fd);
    pid_t child = fork();
    if (child == 0) _exit(0);
    if (child > 0) waitpid(child, NULL, 0);
    printf("fork=%s\n", child < 0 ? "denied" : "allowed");
    /* Independent native spawn API, not an inference from fork denial. */
    char *spawn_args[] = {"/usr/bin/true", NULL};
    pid_t spawned;
    int spawn_result = posix_spawn(&spawned, spawn_args[0], NULL, NULL, spawn_args, environ);
    if (spawn_result == 0) {
        int status;
        if (waitpid(spawned, &status, 0) != spawned || !WIFEXITED(status) || WEXITSTATUS(status) != 0) return 74;
    }
    printf("spawn=%s\n", spawn_result == 0 ? "allowed" : "denied");
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 2 && !strcmp(argv[1], "probe")) return probe(argc, argv);
    if (argc != 2) return 64;
    if (!strcmp(argv[1], "echo")) {
        unsigned char buffer[4096];
        size_t n;
        while ((n = fread(buffer, 1, sizeof buffer, stdin)))
            if (fwrite(buffer, 1, n, stdout) != n) return 74;
        return ferror(stdin) ? 74 : 0;
    }
    if (!strcmp(argv[1], "hang")) {
        FILE *pid = fopen("started.pid", "w");
        if (!pid) return 74;
        fprintf(pid, "%d\n", getpid());
        if (fclose(pid)) return 74;
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
    if (!strcmp(argv[1], "env")) {
        for (char **e = environ; *e; ++e) puts(*e);
        return 0;
    }
    if (!strcmp(argv[1], "fail")) return 9;
    if (!strcmp(argv[1], "invalid_utf8")) {
        unsigned char invalid[] = {0xff, 0xfe};
        return write(1, invalid, sizeof invalid) == sizeof invalid ? 0 : 74;
    }
    if (!strcmp(argv[1], "stdout_flood") || !strcmp(argv[1], "stderr_flood")) {
        int fd = !strcmp(argv[1], "stdout_flood") ? 1 : 2;
        unsigned char buffer[4096];
        memset(buffer, 'X', sizeof buffer);
        for (;;) if (write(fd, buffer, sizeof buffer) < 0) return 74;
    }
    return 64;
}
