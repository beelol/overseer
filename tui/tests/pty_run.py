#!/usr/bin/env python3
"""Runs a command in a pseudo-terminal of a given size, types input after a delay, and prints
everything the command wrote. Exit code: the command's (1 on timeout).
Usage: pty_run.py ROWS COLS DELAY_SECONDS INPUT -- CMD [ARGS...]"""
import fcntl, os, pty, select, struct, sys, termios, time

rows, cols, delay, text = int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3]), sys.argv[4]
cmd = sys.argv[sys.argv.index("--") + 1:]
pid, fd = pty.fork()
if pid == 0:
    os.execvp(cmd[0], cmd)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
out = bytearray()
start = time.time()
sent = False
status = None
while True:
    if not sent and time.time() - start >= delay:
        if text:
            os.write(fd, text.encode())
        sent = True
    r, _, _ = select.select([fd], [], [], 0.05)
    if r:
        try:
            data = os.read(fd, 65536)
        except OSError:
            data = b""
        if not data:
            break
        out += data
    done, st = os.waitpid(pid, os.WNOHANG)
    if done:
        status = st
        try:
            while True:
                r, _, _ = select.select([fd], [], [], 0.1)
                if not r:
                    break
                data = os.read(fd, 65536)
                if not data:
                    break
                out += data
        except OSError:
            pass
        break
    if time.time() - start > delay + 10:
        os.kill(pid, 9)
        break
if status is None:
    _, status = os.waitpid(pid, 0)
sys.stdout.buffer.write(out)
sys.exit(os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else (status >> 8))
