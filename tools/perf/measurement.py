"""Shared process measurement only; fixture semantics belong to each replay."""
import hashlib
from contextlib import contextmanager
import os
from pathlib import Path
import subprocess
import sys
import threading
import time


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def paired_order(binaries, pair):
    names = list(binaries)
    return names if pair % 2 == 0 else list(reversed(names))


def launch_protocol(stable, fixed_executable=False):
    if fixed_executable and not stable:
        raise ValueError('fixed executable slots require stable launch')
    if not stable:
        return 'legacy'
    shell = Path('/bin/zsh')
    if not shell.is_file():
        raise ValueError('--stable-launch requires /bin/zsh')
    if fixed_executable:
        return dict(mode='fixed-executable-slot-v1', argv0='wire', shell=str(shell),
                    shell_sha256=digest(shell), cwd='case-path-execution-directory',
                    executable='execution/wire', staging='hardlink-unlinked-after-exit',
                    artifacts='run-directory/workspace', pe32_log_argument='logs')
    return dict(mode='stable-argv-v1', argv0='wire', shell=str(shell),
                shell_sha256=digest(shell), cwd='run-directory', pe32_log_argument='logs')


def stable_command(command):
    # Only this constant program is interpreted; every supplied argument stays
    # in "$@" and is forwarded literally, including spaces and shell syntax.
    return ['/bin/zsh', '-fc', 'exec -a wire "$@"', 'wire', *map(str, command)]


@contextmanager
def execution(binary, folder, stable=False, fixed_executable=False, expected_sha256=None):
    """Stage only the frozen interpreter; keep every run's workspace afterwards.

    Each case/path uses the same executable name and cwd for every label. The
    hard link is removed after the child exits, before archiving its small
    generated files. No game or other external asset is linked or copied.
    """
    if fixed_executable and not stable:
        raise ValueError('fixed executable slots require stable launch')
    if not fixed_executable:
        yield binary, folder if stable else None
        return
    if expected_sha256 is None:
        raise ValueError('fixed executable slots require a frozen binary hash')
    directory = Path(folder).resolve().parent / 'execution'
    directory.mkdir(exist_ok=False)
    slot = directory / 'wire'
    try:
        os.link(binary, slot)
        if digest(slot) != expected_sha256:
            raise ValueError('staged executable differs from the frozen manifest')
        yield slot, directory
    finally:
        slot.unlink(missing_ok=True)
        directory.rename(Path(folder).resolve() / 'workspace')


def run_command(command, report, timeout=None, *, cwd=None):
    """Preserve failed output/status and measure one child without sampling."""
    with Path(report).open('w') as output:
        start = time.perf_counter()
        process = subprocess.Popen(command, cwd=cwd, stdout=output, stderr=subprocess.STDOUT)
        expired = threading.Event()

        def stop():
            expired.set()
            process.kill()

        timer = threading.Timer(timeout, stop) if timeout is not None else None
        if timer:
            timer.start()
        try:
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        finally:
            if timer:
                timer.cancel()
    result = dict(command=command, exit_code=process.returncode,
                wall_s=time.perf_counter() - start,
                cpu_s=usage.ru_utime + usage.ru_stime,
                peak_rss_bytes=usage.ru_maxrss * (1 if sys.platform == 'darwin' else 1024),
                timed_out=expired.is_set())
    if cwd is not None:
        result['cwd'] = str(Path(cwd).resolve())
    return result
