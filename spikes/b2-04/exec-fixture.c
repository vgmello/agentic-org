#include <fcntl.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    int fd = open(argv[1], O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0) return 3;
    return close(fd) == 0 ? 0 : 4;
}
