//! Internal external-program boundary. No public RPC, native hook or install
//! transaction registers this module; qualified callers/recipes remain later gates.
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub struct Program {
    pub path: PathBuf,
    pub sha256: String,
}

pub struct Request {
    pub program: Program,
    pub scratch: PathBuf,
    pub args: Vec<String>,
    pub deadline: Duration,
    pub max_output_bytes: usize,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Output(Vec<u8>),
    Bypass(&'static str),
}

pub fn run(request: &Request, input: &[u8], cancel: &AtomicBool) -> Outcome {
    #[cfg(target_os = "macos")]
    {
        mac::execute(request, input, cancel, &mut |_, _| {})
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (request, input, cancel);
        Outcome::Bypass("runner_unavailable")
    }
}

// Inert scheduling observation only in the standalone integration test crate.
// No environment/public argument changes program authority or policy.
#[cfg(all(test, target_os = "macos"))]
pub use mac::Point;
#[cfg(all(test, target_os = "macos"))]
pub fn run_observed(
    request: &Request,
    input: &[u8],
    cancel: &AtomicBool,
    observe: &mut dyn FnMut(Point, &std::path::Path),
) -> Outcome {
    mac::execute(request, input, cancel, observe)
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::ffi::CString;
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    const MAX_PROGRAM: u64 = 32 * 1024 * 1024;
    const MAX_INPUT: usize = 1024 * 1024;
    const MAX_OUTPUT: usize = 1024 * 1024;
    type Result<T> = std::result::Result<T, &'static str>;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Point {
        SourceOpened,
        Copied,
        Ready,
    }

    struct Clock<'a> {
        end: Instant,
        cancel: &'a AtomicBool,
    }
    impl Clock<'_> {
        fn check(&self) -> Result<()> {
            if self.cancel.load(Ordering::SeqCst) {
                return Err("cancelled");
            }
            if Instant::now() >= self.end {
                return Err("timeout");
            }
            Ok(())
        }
    }
    fn cstring(bytes: &[u8]) -> Result<CString> {
        CString::new(bytes).map_err(|_| "unsafe_path")
    }
    fn path_parts(path: &Path) -> Result<Vec<&[u8]>> {
        let bytes = path.as_os_str().as_bytes();
        if !bytes.starts_with(b"/")
            || bytes.iter().any(|b| *b < 32 || *b == 127)
            || path.to_str().is_none()
        {
            return Err("unsafe_path");
        }
        let parts: Vec<_> = bytes[1..].split(|b| *b == b'/').collect();
        if parts
            .iter()
            .any(|p| p.is_empty() || *p == b"." || *p == b"..")
        {
            return Err("unsafe_path");
        }
        Ok(parts)
    }
    fn open_at(parent: i32, name: &CString, flags: i32, mode: libc::mode_t) -> Result<File> {
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode,
            )
        };
        if fd < 0 {
            return Err("unsafe_path");
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn trusted_directory(file: &File) -> Result<()> {
        let stat = file.metadata().map_err(|_| "unsafe_path")?;
        let uid = unsafe { libc::geteuid() };
        if !stat.is_dir() || ![0, uid].contains(&stat.uid()) {
            return Err("unsafe_path");
        }
        // Root-owned shared sticky /private/tmp is legitimate. Private leaf
        // directories must instead be owned by this UID and exactly0700.
        let writable = stat.mode() & 0o022 != 0;
        let trusted_sticky = stat.uid() == 0 && stat.mode() & libc::S_ISVTX as u32 != 0;
        if writable && !trusted_sticky {
            return Err("unsafe_path");
        }
        Ok(())
    }
    // Walk actual descriptors; never canonicalize then reopen a source pathname.
    fn open_path(path: &Path, directory: bool, clock: &Clock<'_>) -> Result<File> {
        let parts = path_parts(path)?;
        let root = cstring(b"/")?;
        let fd = unsafe {
            libc::open(
                root.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err("unsafe_path");
        }
        let mut parent = unsafe { File::from_raw_fd(fd) };
        trusted_directory(&parent)?;
        for (index, part) in parts.iter().enumerate() {
            clock.check()?;
            let is_dir = directory || index + 1 < parts.len();
            // An untrusted FIFO/device must not block before fstat can refuse it.
            let flags = libc::O_RDONLY
                | if is_dir {
                    libc::O_DIRECTORY
                } else {
                    libc::O_NONBLOCK
                };
            let next = open_at(parent.as_raw_fd(), &cstring(part)?, flags, 0)?;
            if is_dir {
                trusted_directory(&next)?;
            }
            parent = next;
        }
        Ok(parent)
    }
    fn private_directory(file: &File) -> Result<()> {
        let stat = file.metadata().map_err(|_| "unsafe_path")?;
        if !stat.is_dir()
            || stat.uid() != unsafe { libc::geteuid() }
            || stat.mode() & 0o777 != 0o700
        {
            return Err("unsafe_path");
        }
        Ok(())
    }
    fn same_object(a: &File, b: &File) -> Result<bool> {
        let a = a.metadata().map_err(|_| "unsafe_path")?;
        let b = b.metadata().map_err(|_| "unsafe_path")?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }
    fn chmod(file: &File, mode: libc::mode_t) -> Result<()> {
        if unsafe { libc::fchmod(file.as_raw_fd(), mode) } != 0 {
            return Err("stage_unavailable");
        }
        Ok(())
    }

    struct Stage {
        parent: File,
        dir: File,
        name: CString,
        path: PathBuf,
        program: Option<File>,
    }
    impl Stage {
        fn create(clock: &Clock<'_>) -> Result<Self> {
            let parent = open_path(Path::new("/private/tmp"), true, clock)?;
            let name = cstring(
                format!("overseer-mod-stage-{}", uuid::Uuid::new_v4().simple()).as_bytes(),
            )?;
            if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err("stage_unavailable");
            }
            let dir = match open_at(
                parent.as_raw_fd(),
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            ) {
                Ok(dir) => dir,
                Err(e) => {
                    unsafe {
                        libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR);
                    }
                    return Err(e);
                }
            };
            let stage = Self {
                path: Path::new("/private/tmp").join(name.to_str().unwrap()),
                parent,
                dir,
                name,
                program: None,
            };
            private_directory(&stage.dir)?;
            Ok(stage)
        }
        fn executable(&self) -> PathBuf {
            self.path.join("program")
        }
    }
    impl Drop for Stage {
        fn drop(&mut self) {
            // Only parent-owned staging is removed, never scratch/caller paths.
            let _ = chmod(&self.dir, 0o700);
            let program = CString::new("program").unwrap();
            unsafe {
                libc::unlinkat(self.dir.as_raw_fd(), program.as_ptr(), 0);
                libc::unlinkat(
                    self.parent.as_raw_fd(),
                    self.name.as_ptr(),
                    libc::AT_REMOVEDIR,
                );
            }
        }
    }
    fn stage_program(
        request: &Request,
        clock: &Clock<'_>,
        observer: &mut dyn FnMut(Point, &Path),
    ) -> Result<Stage> {
        let mut source = open_path(&request.program.path, false, clock)?;
        let stat = source.metadata().map_err(|_| "unsafe_path")?;
        if !stat.is_file() || stat.uid() != unsafe { libc::geteuid() } || stat.mode() & 0o022 != 0 {
            return Err("unsafe_path");
        }
        if stat.len() > MAX_PROGRAM {
            return Err("program_limit");
        }
        observer(Point::SourceOpened, &request.program.path);
        clock.check()?;
        let mut stage = Stage::create(clock)?;
        if stage.path.starts_with(&request.scratch) || request.scratch.starts_with(&stage.path) {
            return Err("unsafe_path");
        }
        let name = cstring(b"program")?;
        let mut output = open_at(
            stage.dir.as_raw_fd(),
            &name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let mut copied = 0u64;
        let mut buf = [0u8; 8192];
        loop {
            clock.check()?;
            let n = source.read(&mut buf).map_err(|_| "program_changed")?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            if copied > MAX_PROGRAM {
                return Err("program_limit");
            }
            output
                .write_all(&buf[..n])
                .map_err(|_| "stage_unavailable")?;
        }
        if copied != stat.len() {
            return Err("program_changed");
        }
        observer(Point::Copied, &stage.executable());
        clock.check()?;
        output
            .seek(SeekFrom::Start(0))
            .map_err(|_| "stage_unavailable")?;
        let mut hash = Sha256::new();
        let mut hashed = 0u64;
        loop {
            clock.check()?;
            let n = output.read(&mut buf).map_err(|_| "stage_unavailable")?;
            if n == 0 {
                break;
            }
            hashed += n as u64;
            if hashed > MAX_PROGRAM {
                return Err("program_limit");
            }
            hash.update(&buf[..n]);
        }
        if hashed != copied || format!("{:x}", hash.finalize()) != request.program.sha256 {
            return Err("program_changed");
        }
        chmod(&output, 0o500)?;
        chmod(&stage.dir, 0o500)?;
        // Retain a read-only descriptor to the exact copied inode. Named-entry
        // validation below also refuses replacement of our private staging path.
        // A replaced FIFO must not block before type/identity validation can
        // refuse it, just as with the source and final named-entry opens.
        let readonly = open_at(
            stage.dir.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )?;
        if !readonly
            .metadata()
            .map_err(|_| "program_changed")?
            .is_file()
            || !same_object(&output, &readonly)?
        {
            return Err("program_changed");
        }
        stage.program = Some(readonly);
        // Close writable handles before native execution.
        drop(output);
        drop(source);
        observer(Point::Ready, &stage.executable());
        clock.check()?;
        Ok(stage)
    }

    fn validate_stage(stage: &Stage, expected: &str, clock: &Clock<'_>) -> Result<()> {
        let named_dir = open_path(&stage.path, true, clock)?;
        if !same_object(&stage.dir, &named_dir)? {
            return Err("program_changed");
        }
        let mut named_program = open_at(
            named_dir.as_raw_fd(),
            &cstring(b"program")?,
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )?;
        let held = stage.program.as_ref().ok_or("stage_unavailable")?;
        if !same_object(held, &named_program)? {
            return Err("program_changed");
        }
        let stat = named_program.metadata().map_err(|_| "program_changed")?;
        let dir_stat = named_dir.metadata().map_err(|_| "program_changed")?;
        if !stat.is_file()
            || stat.uid() != unsafe { libc::geteuid() }
            || stat.mode() & 0o777 != 0o500
            || dir_stat.mode() & 0o777 != 0o500
        {
            return Err("program_changed");
        }
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buf = [0; 8192];
        loop {
            clock.check()?;
            let n = named_program
                .read(&mut buf)
                .map_err(|_| "program_changed")?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > MAX_PROGRAM {
                return Err("program_limit");
            }
            hash.update(&buf[..n]);
        }
        if format!("{:x}", hash.finalize()) != expected {
            return Err("program_changed");
        }
        Ok(())
    }

    fn profile_string(path: &Path) -> Result<String> {
        path_parts(path)?;
        Ok(format!(
            "\"{}\"",
            path.to_str()
                .ok_or("unsafe_path")?
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        ))
    }
    fn policy(stage: &Stage, scratch: &Path) -> Result<String> {
        let executable = profile_string(&stage.executable())?;
        // Exact ancestor metadata permits path lookup without granting content
        // reads from unrelated directories. No developer/repository subtree.
        let mut metadata = std::collections::BTreeSet::new();
        for path in [stage.executable(), scratch.to_path_buf()] {
            for ancestor in path.ancestors() {
                if ancestor == Path::new("/") {
                    continue;
                }
                metadata.insert(format!("(literal {})", profile_string(ancestor)?));
            }
        }
        let metadata = metadata.into_iter().collect::<Vec<_>>().join(" ");
        let scratch = profile_string(scratch)?;
        // System dyld/native libraries only, not a developer toolchain/home.
        // Extra runtime allowances require actual scoped helper evidence.
        Ok(format!(
            r#"(version 1)
(deny default)
(deny process-fork)
(allow process-exec (literal {executable}))
(allow file-read-data (literal {executable}) (subpath "/usr/lib") (subpath "/System/Library/dyld"))
(allow file-read-metadata (literal "/") (literal "/private") (literal "/private/tmp") (literal "/usr") (literal "/usr/lib") (literal "/System") (literal "/System/Library") (subpath "/usr/lib") (subpath "/System/Library/dyld") (literal {executable}) {metadata})
(allow file-read* (subpath {scratch}))
(allow file-write* (subpath {scratch}))
(allow file-read-data (literal "/dev/null") (literal "/dev/urandom") (literal "/dev/random"))
"#
        ))
    }

    struct Group {
        child: Child,
    }
    impl Drop for Group {
        fn drop(&mut self) {
            let pid = self.child.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = self.child.kill();
            // No live process wait before SIGKILL. Reap is synchronous on every
            // return; stage outlives this guard until the owned process ends.
            let _ = self.child.wait();
        }
    }
    fn nonblocking(fd: i32) -> Result<()> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err("pipe_failure");
        }
        Ok(())
    }
    fn drain<R: Read>(
        reader: &mut Option<R>,
        bytes: &mut Vec<u8>,
        total: &mut usize,
        limit: usize,
        clock: &Clock<'_>,
    ) -> Result<()> {
        let mut buf = [0u8; 8192];
        while let Some(stream) = reader.as_mut() {
            clock.check()?;
            match stream.read(&mut buf) {
                Ok(0) => {
                    *reader = None;
                    break;
                }
                Ok(n) => {
                    if n > limit.saturating_sub(*total) {
                        return Err("output_limit");
                    }
                    *total += n;
                    bytes.extend_from_slice(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("pipe_failure"),
            }
        }
        Ok(())
    }
    fn pipes(mut group: Group, input: &[u8], limit: usize, clock: &Clock<'_>) -> Result<Vec<u8>> {
        let mut stdin = group.child.stdin.take();
        let mut stdout = group.child.stdout.take();
        let mut stderr = group.child.stderr.take();
        nonblocking(stdin.as_ref().ok_or("pipe_failure")?.as_raw_fd())?;
        nonblocking(stdout.as_ref().ok_or("pipe_failure")?.as_raw_fd())?;
        nonblocking(stderr.as_ref().ok_or("pipe_failure")?.as_raw_fd())?;
        let mut written = 0usize;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut total = 0usize;
        let mut status = None;
        loop {
            clock.check()?;
            if written == input.len() {
                stdin = None;
            }
            if let Some(stream) = stdin.as_mut() {
                match stream.write(&input[written..]) {
                    Ok(0) => return Err("input_incomplete"),
                    Ok(n) => written += n,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                        return Err("input_incomplete")
                    }
                    Err(_) => return Err("pipe_failure"),
                }
            }
            drain(&mut stdout, &mut out, &mut total, limit, clock)?;
            drain(&mut stderr, &mut err, &mut total, limit, clock)?;
            if status.is_none() {
                status = group.child.try_wait().map_err(|_| "process_failure")?;
            }
            if let Some(status) = status {
                if !status.success() {
                    return Err("exit_failure");
                }
                if stdout.is_none() && stderr.is_none() {
                    if written != input.len() {
                        return Err("input_incomplete");
                    }
                    if std::str::from_utf8(&out).is_err() {
                        return Err("invalid_text");
                    }
                    return Ok(out);
                }
            }
            let mut pollfds = Vec::new();
            if let Some(stream) = &stdin {
                pollfds.push(libc::pollfd {
                    fd: stream.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                });
            }
            if let Some(stream) = &stdout {
                pollfds.push(libc::pollfd {
                    fd: stream.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                });
            }
            if let Some(stream) = &stderr {
                pollfds.push(libc::pollfd {
                    fd: stream.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                });
            }
            let millis = clock
                .end
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(10) as i32;
            let result =
                unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, millis) };
            if result < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return Err("pipe_failure");
            }
        }
    }

    fn inner(
        request: &Request,
        input: &[u8],
        cancel: &AtomicBool,
        observer: &mut dyn FnMut(Point, &Path),
    ) -> Result<Vec<u8>> {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled");
        }
        let end = Instant::now()
            .checked_add(request.deadline)
            .ok_or("timeout")?;
        let clock = Clock { end, cancel };
        clock.check()?;
        if input.len() > MAX_INPUT {
            return Err("input_limit");
        }
        if request.max_output_bytes == 0
            || request.max_output_bytes > MAX_OUTPUT
            || request.args.len() > 64
            || request.args.iter().map(String::len).sum::<usize>() > 8192
        {
            return Err("invalid_request");
        }
        if request.program.sha256.len() != 64
            || !request
                .program
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err("program_changed");
        }
        let source_parent = request.program.path.parent().ok_or("unsafe_path")?;
        if source_parent.starts_with(&request.scratch) || request.scratch.starts_with(source_parent)
        {
            return Err("unsafe_path");
        }
        let scratch = open_path(&request.scratch, true, &clock)?;
        private_directory(&scratch)?;
        let stage = stage_program(request, &clock, observer)?;
        // Revalidate the named scratch before policy creation. fchdir below
        // uses the held descriptor, so a rename cannot change the child cwd.
        let named_scratch = open_path(&request.scratch, true, &clock)?;
        if !same_object(&scratch, &named_scratch)? {
            return Err("unsafe_path");
        }
        let profile = policy(&stage, &request.scratch)?;
        validate_stage(&stage, &request.program.sha256, &clock)?;
        clock.check()?;
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command
            .arg("-p")
            .arg(profile)
            .arg(stage.executable())
            .args(&request.args)
            .env_clear()
            .env("HOME", &request.scratch)
            .env("TMPDIR", &request.scratch)
            .env("PATH", "/usr/bin:/bin")
            .env("RTK_TELEMETRY_DISABLED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let cwd_fd = scratch.as_raw_fd();
        unsafe {
            command.pre_exec(move || {
                if libc::fchdir(cwd_fd) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let group = Group {
            child: command.spawn().map_err(|_| "runner_unavailable")?,
        };
        clock.check()?;
        pipes(group, input, request.max_output_bytes, &clock)
    }
    pub fn execute(
        request: &Request,
        input: &[u8],
        cancel: &AtomicBool,
        observer: &mut dyn FnMut(Point, &Path),
    ) -> Outcome {
        match inner(request, input, cancel, observer) {
            Ok(bytes) => Outcome::Output(bytes),
            Err(reason) => Outcome::Bypass(reason),
        }
    }
}
