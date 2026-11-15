# PROTOTYPE. Runs inside the sandbox and tries to exceed the reach ADR 4 promises.
import ctypes, os, socket, signal, sys

OUT, RO, WS = os.environ["OUT"], os.environ["RO"], os.environ["WS"]
VICTIM, SOCK = int(os.environ["VICTIM_PID"]), os.environ["SOCK"]
libc = ctypes.CDLL(None, use_errno=True)

def attempt(name, fn):
    try:
        fn()
        print(f"  ALLOWED  {name}")
    except OSError as e:
        print(f"  blocked  {name}  ({os.strerror(e.errno)})")

def ptrace_attach():
    if libc.ptrace(16, VICTIM, 0, 0) != 0:  # PTRACE_ATTACH
        raise OSError(ctypes.get_errno(), "ptrace")
    libc.ptrace(17, VICTIM, 0, 0)  # PTRACE_DETACH

def connect(family, addr):
    s = socket.socket(family, socket.SOCK_STREAM)
    s.connect(addr)

secret = f"{OUT}/secret"
attempt("read a file outside reach", lambda: open(secret).read())
attempt("create a file outside reach", lambda: open(f"{OUT}/new", "w"))
attempt("hard-link an outside file into the workspace", lambda: os.link(secret, f"{WS}/stolen"))
attempt("write a read-only path", lambda: open(f"{RO}/file", "a"))
attempt("truncate a read-only path", lambda: os.truncate(f"{RO}/file", 0))
attempt("chmod a file outside reach", lambda: os.chmod(secret, 0o644))
attempt("chmod a read-only path", lambda: os.chmod(f"{RO}/file", 0o666))
attempt("chown a file outside reach", lambda: os.chown(secret, os.getuid(), os.getgid()))
attempt("set mtime on a file outside reach", lambda: os.utime(secret, (0, 0)))
attempt("setxattr on a file outside reach", lambda: os.setxattr(secret, "user.sbx", b"1"))
attempt("signal a same-user process outside the sandbox", lambda: os.kill(VICTIM, signal.SIGCONT))
attempt("ptrace a same-user process outside the sandbox", ptrace_attach)
attempt("read that process's environment in /proc", lambda: open(f"/proc/{VICTIM}/environ").read())
attempt("create an internet socket", lambda: socket.socket(socket.AF_INET, socket.SOCK_DGRAM))
attempt("create a netlink socket", lambda: socket.socket(socket.AF_NETLINK, socket.SOCK_RAW))
attempt("connect to a Unix socket file outside reach", lambda: connect(socket.AF_UNIX, SOCK))
attempt("connect to an abstract Unix socket", lambda: connect(socket.AF_UNIX, "\0sbx-probe"))
