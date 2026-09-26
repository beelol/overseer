#!/usr/bin/env python3
"""Runs a command in a pseudo-terminal of a given size, types timed input, and prints everything
the command wrote. Exit code: the command's (1 on timeout).
Usage: pty_run.py ROWS COLS [SECONDS INPUT]... -- CMD [ARGS...]
Each INPUT is typed SECONDS after the previous one."""
import fcntl, os, pty, select, struct, sys, termios, time

rows, cols = int(sys.argv[1]), int(sys.argv[2])
sep = sys.argv.index("--")
pairs = sys.argv[3:sep]
steps = [(float(pairs[i]), pairs[i + 1]) for i in range(0, len(pairs), 2)]
cmd = sys.argv[sep + 1:]
delay = sum(d for d, _ in steps)
pid, fd = pty.fork()
if pid == 0:
    os.execvp(cmd[0], cmd)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
out = bytearray()
start = time.time()
step = 0
due = start + (steps[0][0] if steps else 0)
status = None
while True:
    if step < len(steps) and time.time() >= due:
        if steps[step][1]:
            os.write(fd, steps[step][1].encode())
        step += 1
        if step < len(steps):
            due = time.time() + steps[step][0]
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
    if time.time() - start > delay + 15:
        os.kill(pid, 9)
        break
if status is None:
    _, status = os.waitpid(pid, 0)
sys.stdout.buffer.write(out)
sys.exit(os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else (status >> 8))
