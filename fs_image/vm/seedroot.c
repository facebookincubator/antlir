// this is a very rigid re-implementation of what btrfs device add would do to
// add a rw device to a mount backed by a seed device
// since it's so few lines of code it is preferable in the highly controlled
// environment of vmtest to do this instead of spending minutes installing
// btrfs-progs in images for every single kernel that is being tested
#include <asm/types.h>
#include <dirent.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

#define BTRFS_IOCTL_MAGIC 0x94
#define BTRFS_PATH_NAME_MAX 4087
struct btrfs_ioctl_vol_args {
  __s64 fd;
  char name[BTRFS_PATH_NAME_MAX + 1];
};

#define BTRFS_IOC_ADD_DEV2 \
  _IOW(BTRFS_IOCTL_MAGIC, 10, struct btrfs_ioctl_vol_args)

int main(int argc, char** argv) {
  char* mntpnt = "/newroot";
  char* dev = "/dev/vdb";
  struct btrfs_ioctl_vol_args ioctl_args;
  int res;
  DIR* dir = NULL;
  int fdmnt = 0;

  dir = opendir(mntpnt);
  if (dir == NULL) {
    fprintf(stderr, "error opening mount '%s': %m\n", mntpnt);
    return 1;
  }
  fdmnt = dirfd(dir);

  memset(&ioctl_args, 0, sizeof(ioctl_args));
  strcpy(ioctl_args.name, dev);
  res = ioctl(fdmnt, BTRFS_IOC_ADD_DEV2, &ioctl_args);
  if (res < 0) {
    fprintf(stderr, "error adding device '%s': %m\n", dev);
    return 1;
  }
  close(fdmnt);
  closedir(dir);

  return 0;
}
