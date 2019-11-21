#!/bin/bash
# This script requires the following things to be done by the process that
# invokes this:
#  1) Mount /proc from the namespace of the caller to /outerproc *inside* the
#     the container root.
#  2) Copy this script into the container root somewhere.  It doesn't matter
#     where but it must be copied because it will attempt to remove itself.
#  3) Invoke this script with with a writable FD forwarded into the namespace
#     this is being executed in as fd #3.

# Get the parent pid of the 'grep' process which will be the pid of this
# script and eventually the pid of systemd.
grep ^PPid: /outerproc/self/status >&3

# NB:  We don't close the forwarded FD in this wrapper.  Instead we rely on
# systemd to close all FD's it doesn't know about during it's initialization
# sequence.  We rely on this because if we don't, then systemd may not have
# had time to setup the necessary signal handlers to process the SIGRTMIN+3
# shutdown signal that happens after a command is invoked.

# Cleanup the evidence of this wrapper.  This is done so that users writing 
# tests don't have to concern themselves with the runtime details of this
# process.  If we left these things around users would need to compensate
# for them when validating things like files on disk or mount tables.
umount -R /outerproc && rmdir /outerproc && rm "$0"

# Future: pass the thing to exec into as an argument or as an environment
# variable.
exec -a /usr/lib/systemd/systemd /usr/lib/systemd/systemd
