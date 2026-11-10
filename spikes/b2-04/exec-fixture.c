#include <fcntl.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char authority[4];
    if (read(STDIN_FILENO, authority, sizeof(authority)) != sizeof(authority)) return 5;
    if (authority[0] != 't' || authority[1] != 'e' || authority[2] != 's' || authority[3] != 't') return 6;
    int fd = open(argv[1], O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0) return 3;
    return close(fd) == 0 ? 0 : 4;
}
