use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use anyhow::Result;
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use nix::sched::{setns, unshare, CloneFlags};
use nix::unistd::gettid;

/// Defines a NetNs environment behavior.
pub trait Env {
    /// Initialize the environment.
    fn init(&self) -> Result<()>;

    fn contains<P: AsRef<Path>>(self: &std::sync::Arc<Self>, ns_path: P) -> bool;

    fn create<P: AsRef<Path>>(
        self: &std::sync::Arc<Self>,
        ns_path: P,
    ) -> Result<std::sync::Arc<NetNs<Self>>>
    where
        Self: Sized;

    fn remove(self: &std::sync::Arc<Self>, ns_path: &mut NetNs<Self>) -> Result<()>
    where
        Self: Sized;

    fn current(self: &std::sync::Arc<Self>) -> Result<std::sync::Arc<NetNs<Self>>>
    where
        Self: Sized;
}

/// A default network namespace environment.
///
/// Its persistence directory is `/var/run/netns`, which is for consistency with the `ip-netns` tool.
/// See [ip-netns](https://man7.org/linux/man-pages/man8/ip-netns.8.html) for details.
#[derive(Copy, Clone, Default, Debug)]
pub struct DefaultEnv;

/// path argument to functions defined here is prefixed with self.persist_dir()
impl DefaultEnv {
    fn persist_dir(&self) -> PathBuf {
        PathBuf::from("/var/run/netns")
    }

    fn umount_ns<P: AsRef<Path>>(path: P) -> Result<()> {
        let path = path.as_ref();
        umount2(path, MntFlags::MNT_DETACH)
            .map_err(|e| anyhow::anyhow!(format!("unable to umount {}", path.display())))?;
        let _ = std::fs::remove_file(path);
        Ok(())
    }

    fn persistent_internal<P: AsRef<Path>>(ns_path: P) -> Result<()> {
        // create an empty file at the mount point
        let _ = File::create(&ns_path)?;

        // Create a new netns for the current thread.
        unshare(CloneFlags::CLONE_NEWNET)?;
        // bind mount the netns from the current thread (from /proc) onto the mount point.
        // This persists the ns, even when there are no threads in the ns.
        let src = Self::get_current_netns_path();
        mount(
            Some(src.as_path()),
            ns_path.as_ref(),
            Some("none"),
            MsFlags::MS_BIND,
            Some(""),
        )
        .map_err(|_| {
            anyhow::anyhow!(format!(
                "(BIND) {} to {}",
                src.display(),
                ns_path.as_ref().display()
            ))
        })?;

        Ok(())
    }

    fn persistent<P: AsRef<Path>>(&self, ns_path: P) -> Result<()> {
        let path = ns_path.as_ref().to_owned();
        let new_thread: JoinHandle<Result<()>> =
            thread::spawn(move || Self::persistent_internal(path));
        match new_thread.join() {
            Ok(t) => match t {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            Err(e) => Err(anyhow::anyhow!(format!("{:?}", e))),
        }
    }

    #[inline]
    fn get_current_netns_path() -> PathBuf {
        PathBuf::from(format!("/proc/self/task/{}/ns/net", gettid()))
    }
}

impl Env for DefaultEnv {
    /// Initialize the environment.
    fn init(&self) -> Result<()> {
        // Create the directory for mounting network namespaces.
        // This needs to be a shared mount-point in case it is mounted in to
        // other namespaces (containers)
        let persist_dir = self.persist_dir();
        std::fs::create_dir_all(&persist_dir).unwrap();

        // Remount the namespace directory shared. This will fail if it is not
        // already a mount-point, so bind-mount it on to itself to "upgrade" it
        // to a mount-point.
        let mut made_netns_persist_dir_mount: bool = false;
        while let Err(e) = mount(
            Some(""),
            &persist_dir,
            Some("none"),
            MsFlags::MS_SHARED | MsFlags::MS_REC,
            Some(""),
        ) {
            // Fail unless we need to make the mount point
            if e != nix::errno::Errno::EINVAL || made_netns_persist_dir_mount {
                return Err(anyhow::anyhow!(format!(
                    "(SHARED|REC) {}",
                    persist_dir.display()
                )));
            }
            // Recursively remount /var/<persist> on itself. The recursive flag is
            // so that any existing netns bind-mounts are carried over.
            mount(
                Some(&persist_dir),
                &persist_dir,
                Some("none"),
                MsFlags::MS_BIND | MsFlags::MS_REC,
                Some(""),
            )
            .map_err(|_| {
                anyhow::anyhow!(format!(
                    "(BIND|REC) {} to {}",
                    persist_dir.display(),
                    persist_dir.display()
                ),)
            })
            .unwrap();
            made_netns_persist_dir_mount = true;
        }
        Ok(())
    }

    /// Returns `true` if the given path is in this Env.
    fn contains<P: AsRef<Path>>(self: &std::sync::Arc<Self>, p: P) -> bool {
        p.as_ref().starts_with(self.persist_dir())
    }

    fn create<P: AsRef<Path>>(
        self: &std::sync::Arc<Self>,
        ns_path: P,
    ) -> Result<std::sync::Arc<NetNs>> {
        let full_path = self.persist_dir().join(ns_path.as_ref());
        self.persistent(&full_path)?;

        let file = File::open(&full_path)?;

        Ok(std::sync::Arc::new(NetNs {
            file,
            path: full_path,
            env: self.clone(),
        }))
    }

    fn remove(self: &std::sync::Arc<Self>, netns: &mut NetNs) -> Result<()> {
        let path = &netns.path;
        if path.starts_with(self.persist_dir()) {
            println!("drop namespace: {}", netns.path().to_string_lossy());
            Self::umount_ns(path)?
        }
        Ok(())
    }

    /// Returns the NetNs of current thread.
    fn current(self: &std::sync::Arc<Self>) -> Result<std::sync::Arc<NetNs>> {
        let ns_path = Self::get_current_netns_path();
        let file = File::open(&ns_path).unwrap();

        Ok(NetNs {
            file,
            path: ns_path,
            env: self.clone(),
        }
        .into())
    }
}

/// A network namespace type.
///
/// It could be used to enter network namespace.
#[derive(Debug)]
pub struct NetNs<E: Env = DefaultEnv> {
    /// the open file descriptor of the network namespace
    file: File,
    /// the path of the network namespace
    /// it could be /proc/self/task/{}/ns/net or /var/run/netns/<name>
    path: PathBuf,
    /// the environment manage the network namespace
    env: std::sync::Arc<E>,
}

impl<E: Env> AsRawFd for NetNs<E> {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.file.as_raw_fd()
    }
}

impl<E: Env> std::fmt::Display for NetNs<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if let Ok(meta) = self.file.metadata() {
            write!(
                f,
                "NetNS {{ fd: {}, dev: {}, ino: {}, path: {} }}",
                self.as_raw_fd(),
                meta.dev(),
                meta.ino(),
                self.path.display()
            )
        } else {
            write!(
                f,
                "NetNS {{ fd: {}, path: {} }}",
                self.as_raw_fd(),
                self.path.display()
            )
        }
    }
}

impl<E1: Env, E2: Env> PartialEq<NetNs<E1>> for NetNs<E2> {
    fn eq(&self, other: &NetNs<E1>) -> bool {
        if self.as_raw_fd() == other.as_raw_fd() {
            return true;
        }
        let cmp_meta = |f1: &File, f2: &File| -> Option<bool> {
            let m1 = match f1.metadata() {
                Ok(m) => m,
                Err(_) => return None,
            };
            let m2 = match f2.metadata() {
                Ok(m) => m,
                Err(_) => return None,
            };
            Some(m1.dev() == m2.dev() && m1.ino() == m2.ino())
        };
        cmp_meta(&self.file, &other.file).unwrap_or_else(|| self.path == other.path)
    }
}

impl<E: Env> NetNs<E> {
    /// Creates a new `NetNs` with the specified name and Env.
    ///
    /// The persist dir of network namespace will be created if it doesn't already exist.
    pub fn new_with_env<S: AsRef<str>>(
        ns_name: S,
        env: std::sync::Arc<E>,
    ) -> Result<std::sync::Arc<Self>> {
        env.create(Path::new(ns_name.as_ref()))
    }

    /// Makes the current thread enter this network namespace.
    ///
    /// Requires elevated privileges.
    pub fn enter(&self) -> Result<NetNsGuard<E>> {
        let current_ns = self.env.clone().current().unwrap();
        setns(self.as_raw_fd(), CloneFlags::CLONE_NEWNET).unwrap();
        Ok(NetNsGuard { old: current_ns })
    }

    fn enter_without_guard(&self) -> Result<()> {
        setns(self.as_raw_fd(), CloneFlags::CLONE_NEWNET).unwrap();
        Ok(())
    }

    /// Gets the path of this NetNs.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Gets the Env of this NetNs.
    pub fn env(&self) -> std::sync::Arc<E> {
        self.env.clone()
    }

    /// Gets the Env of this network namespace.
    pub fn file(&self) -> &File {
        &self.file
    }
}

impl<E: Env> Drop for NetNs<E> {
    fn drop(&mut self) {
        let fd = self.file.as_raw_fd();
        if let Err(e) = nix::unistd::close(fd) {
            eprintln!("Failed to close netns: {}", e);
        }
        if let Err(e) = self.env.clone().remove(self) {
            eprintln!("Failed to remove netns: {}", e);
        }
    }
}

pub struct NetNsGuard<E: Env = DefaultEnv> {
    old: std::sync::Arc<NetNs<E>>,
}

impl<E> Drop for NetNsGuard<E>
where
    E: Env,
{
    fn drop(&mut self) {
        if let Err(e) = self.old.enter_without_guard() {
            panic!("Failed to go back to old netns: {}", e);
        }
    }
}

impl NetNs {
    /// Creates a new persistent (bind-mounted) network namespace and returns an object representing
    /// that namespace, without switching to it. Report an error if the namespace already exists.
    ///
    /// The persist directory of network namespace will be created if it doesn't already exist.
    /// This function will use [`DefaultEnv`] to create persist directory.
    ///
    /// Requires elevated privileges.
    ///
    /// [`DefaultEnv`]: DefaultEnv
    ///
    pub fn new<S: AsRef<str>>(ns_name: S) -> Result<std::sync::Arc<Self>> {
        let default_env = std::sync::Arc::new(DefaultEnv);
        default_env.init()?;
        Self::new_with_env(ns_name, default_env)
    }

    pub fn current() -> Result<std::sync::Arc<Self>> {
        let default_env = std::sync::Arc::new(DefaultEnv);
        default_env.init()?;
        default_env.current()
    }
}
