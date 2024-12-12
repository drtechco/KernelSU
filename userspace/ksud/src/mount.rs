use std::ffi::CString;
use anyhow::{anyhow, bail, Ok, Result};

#[cfg(any(target_os = "linux", target_os = "android"))]
use anyhow::Context;
#[cfg(any(target_os = "linux", target_os = "android"))]
use rustix::{fd::AsFd, fs::CWD, mount::*};

use crate::defs::KSU_OVERLAY_SOURCE;
use log::{info, warn};
#[cfg(any(target_os = "linux", target_os = "android"))]
use procfs::process::Process;
use std::path::Path;
use std::path::PathBuf;

pub struct AutoMountExt4 {
    target: String,
    auto_umount: bool,
}

impl AutoMountExt4 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn try_new(source: &str, target: &str, auto_umount: bool) -> Result<Self> {
        mount_ext4(source, target)?;
        Ok(Self {
            target: target.to_string(),
            auto_umount,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn try_new(_src: &str, _mnt: &str, _auto_umount: bool) -> Result<Self> {
        unimplemented!()
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn umount(&self) -> Result<()> {
        unmount(self.target.as_str(), UnmountFlags::DETACH)?;
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl Drop for AutoMountExt4 {
    fn drop(&mut self) {
        log::info!(
            "AutoMountExt4 drop: {}, auto_umount: {}",
            self.target,
            self.auto_umount
        );
        if self.auto_umount {
            let _ = self.umount();
        }
    }
}

fn mount_ext4_modern(source: &Path, target: &Path) -> Result<()> {
    println!("[modern] Start mounting ext4 from {:?} to {:?}", source, target);

    // Create and setup loop device
    let new_loopback = loopdev::LoopControl::open()?
        .next_free()
        .with_context(|| "Failed to alloc loop")?;
    println!("[modern] Allocated loop device: {:?}", new_loopback.path());
    new_loopback
        .with()
        .attach(source)
        .with_context(|| format!("Failed to attach loop device to {:?}", source))?;

    let lo = new_loopback
        .path()
        .ok_or(anyhow!("no loop"))?;
    println!("[modern] Attached loop device: {:?}", lo);

    // Try modern mount (fsopen)
    let fs = fsopen("ext4", FsOpenFlags::FSOPEN_CLOEXEC)?;
    println!("[modern] fsopen succeeded");
    let fs_fd = fs.as_fd();

    // Configure mount source
    let src_str = lo.to_str().ok_or(anyhow!("Invalid loop path"))?;
    println!("[modern] Configuring mount source: {}", src_str);
    fsconfig_set_string(fs_fd, "source", src_str)?;

    // Create filesystem configuration
    fsconfig_create(fs_fd)?;
    println!("[modern] fsconfig_create succeeded");

    // Create mount
    let mount = fsmount(fs_fd, FsMountFlags::FSMOUNT_CLOEXEC, MountAttrFlags::empty())?;
    println!("[modern] fsmount succeeded");

    // Move mount to target location
    println!("[modern] move_mount to {:?}", target);
    move_mount(
        mount.as_fd(),
        "",
        CWD,
        target,
        MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
    )?;

    println!("[modern] mount_ext4_modern done");
    Ok(())
}
unsafe fn mount_ext4_fallback(source: &Path, target: &Path) -> Result<()> {
    println!("[fallback] Start mounting ext4 from {:?} to {:?}", source, target);

    // 创建并设置环回设备(loop device)
    let new_loopback = loopdev::LoopControl::open()?
        .next_free()
        .with_context(|| "Failed to alloc loop")?;
    println!("[fallback] Allocated loop device: {:?}", new_loopback.path());

    new_loopback
        .with()
        .attach(source)
        .with_context(|| format!("Failed to attach loop device to {:?}", source))?;

    let lo = new_loopback
        .path()
        .ok_or(anyhow!("no loop"))?;
    println!("[fallback] Attached loop device: {:?}", lo);

    let source_c = std::ffi::CString::new(lo.to_str().ok_or(anyhow!("Invalid source path"))?)?;
    let target_c = std::ffi::CString::new(target.to_str().ok_or(anyhow!("Invalid target path"))?)?;
    let fstype = std::ffi::CString::new("ext4")?;

    // 确保目标目录存在
    std::fs::create_dir_all(target)?;

    // 使用传统mount系统调用，直接将环回设备挂载到 `target`
    let ret = libc::mount(
        source_c.as_ptr(),
        target_c.as_ptr(),
        fstype.as_ptr(),
        0,
        std::ptr::null(),
    );

    if ret != 0 {
        let err = std::io::Error::last_os_error();
        bail!("[fallback] mount failed: {}", err);
    }

    println!("[fallback] mount_ext4_fallback done: {} is now mounted on {}",
             lo.to_string_lossy(),
             target.to_string_lossy()
    );

    Ok(())
}

// Helper function to copy directory contents recursively
fn copy_dir_contents(from: &Path, to: &Path) -> Result<()> {
    if !to.exists() {
        std::fs::create_dir_all(to)?;
    }

    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = to.join(entry.file_name());

        if path.is_dir() {
            copy_dir_contents(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }

    Ok(())
}
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn mount_ext4(source: impl AsRef<Path>, target: impl AsRef<Path>) -> anyhow::Result<()> {
    let source = source.as_ref();
    let target = target.as_ref();
    println!("[mount_ext4] Attempting modern mount first");
    let result = mount_ext4_modern(source, target);
    if result.is_err() {
        let err = result.err();
        println!("[mount_ext4] Modern mount failed: {:?}", err);
        // If ENOSYS occurs, fall back to the old method
        println!("[mount_ext4] fsopen not supported, fallback to old method");
        return unsafe { mount_ext4_fallback(source, target) };
    } else {
        println!("[mount_ext4] Modern mount succeeded");
        Ok(())
    }
}
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn umount_dir(src: impl AsRef<Path>) -> Result<()> {
    unmount(src.as_ref(), UnmountFlags::empty())
        .with_context(|| format!("Failed to umount {}", src.as_ref().display()))?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn mount_overlayfs(
    lower_dirs: &[String],
    lowest: &str,
    upperdir: Option<PathBuf>,
    workdir: Option<PathBuf>,
    dest: impl AsRef<Path>,
) -> Result<()> {
    let lowerdir_config = lower_dirs
        .iter()
        .map(|s| s.as_ref())
        .chain(std::iter::once(lowest))
        .collect::<Vec<_>>()
        .join(":");
    info!(
        "mount overlayfs on {:?}, lowerdir={}, upperdir={:?}, workdir={:?}",
        dest.as_ref(),
        lowerdir_config,
        upperdir,
        workdir
    );

    let upperdir = upperdir
        .filter(|up| up.exists())
        .map(|e| e.display().to_string());
    let workdir = workdir
        .filter(|wd| wd.exists())
        .map(|e| e.display().to_string());

    let result = (|| {
        let fs = fsopen("overlay", FsOpenFlags::FSOPEN_CLOEXEC)?;
        let fs = fs.as_fd();
        fsconfig_set_string(fs, "lowerdir", &lowerdir_config)?;
        if let (Some(upperdir), Some(workdir)) = (&upperdir, &workdir) {
            fsconfig_set_string(fs, "upperdir", upperdir)?;
            fsconfig_set_string(fs, "workdir", workdir)?;
        }
        fsconfig_set_string(fs, "source", KSU_OVERLAY_SOURCE)?;
        fsconfig_create(fs)?;
        let mount = fsmount(fs, FsMountFlags::FSMOUNT_CLOEXEC, MountAttrFlags::empty())?;
        move_mount(
            mount.as_fd(),
            "",
            CWD,
            dest.as_ref(),
            MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
        )
    })();

    if let Err(e) = result {
        warn!("fsopen mount failed: {:#}, fallback to mount", e);
        let mut data = format!("lowerdir={lowerdir_config}");
        if let (Some(upperdir), Some(workdir)) = (upperdir, workdir) {
            data = format!("{data},upperdir={upperdir},workdir={workdir}");
        }
        mount(
            KSU_OVERLAY_SOURCE,
            dest.as_ref(),
            "overlay",
            MountFlags::empty(),
            data,
        )?;
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn mount_tmpfs(dest: impl AsRef<Path>) -> Result<()> {
    info!("mount tmpfs on {}", dest.as_ref().display());
    let fs = fsopen("tmpfs", FsOpenFlags::FSOPEN_CLOEXEC)?;
    let fs = fs.as_fd();
    fsconfig_set_string(fs, "source", KSU_OVERLAY_SOURCE)?;
    fsconfig_create(fs)?;
    let mount = fsmount(fs, FsMountFlags::FSMOUNT_CLOEXEC, MountAttrFlags::empty())?;
    move_mount(
        mount.as_fd(),
        "",
        CWD,
        dest.as_ref(),
        MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
    )?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn bind_mount(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    info!(
        "bind mount {} -> {}",
        from.as_ref().display(),
        to.as_ref().display()
    );
    let tree = open_tree(
        CWD,
        from.as_ref(),
        OpenTreeFlags::OPEN_TREE_CLOEXEC
            | OpenTreeFlags::OPEN_TREE_CLONE
            | OpenTreeFlags::AT_RECURSIVE,
    )?;
    move_mount(
        tree.as_fd(),
        "",
        CWD,
        to.as_ref(),
        MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
    )?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn mount_overlay_child(
    mount_point: &str,
    relative: &String,
    module_roots: &Vec<String>,
    stock_root: &String,
) -> Result<()> {
    if !module_roots
        .iter()
        .any(|lower| Path::new(&format!("{lower}{relative}")).exists())
    {
        return bind_mount(stock_root, mount_point);
    }
    if !Path::new(&stock_root).is_dir() {
        return Ok(());
    }
    let mut lower_dirs: Vec<String> = vec![];
    for lower in module_roots {
        let lower_dir = format!("{lower}{relative}");
        let path = Path::new(&lower_dir);
        if path.is_dir() {
            lower_dirs.push(lower_dir);
        } else if path.exists() {
            // stock root has been blocked by this file
            return Ok(());
        }
    }
    if lower_dirs.is_empty() {
        return Ok(());
    }
    // merge modules and stock
    if let Err(e) = mount_overlayfs(&lower_dirs, stock_root, None, None, mount_point) {
        warn!("failed: {:#}, fallback to bind mount", e);
        bind_mount(stock_root, mount_point)?;
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn mount_overlay(
    root: &String,
    module_roots: &Vec<String>,
    workdir: Option<PathBuf>,
    upperdir: Option<PathBuf>,
) -> Result<()> {
    info!("mount overlay for {}", root);
    std::env::set_current_dir(root).with_context(|| format!("failed to chdir to {root}"))?;
    let stock_root = ".";

    // collect child mounts before mounting the root
    let mounts = Process::myself()?
        .mountinfo()
        .with_context(|| "get mountinfo")?;
    let mut mount_seq = mounts
        .0
        .iter()
        .filter(|m| {
            m.mount_point.starts_with(root) && !Path::new(&root).starts_with(&m.mount_point)
        })
        .map(|m| m.mount_point.to_str())
        .collect::<Vec<_>>();
    mount_seq.sort();
    mount_seq.dedup();

    mount_overlayfs(module_roots, root, upperdir, workdir, root)
        .with_context(|| "mount overlayfs for root failed")?;
    for mount_point in mount_seq.iter() {
        let Some(mount_point) = mount_point else {
            continue;
        };
        let relative = mount_point.replacen(root, "", 1);
        let stock_root: String = format!("{stock_root}{relative}");
        if !Path::new(&stock_root).exists() {
            continue;
        }
        if let Err(e) = mount_overlay_child(mount_point, &relative, module_roots, &stock_root) {
            warn!(
                "failed to mount overlay for child {}: {:#}, revert",
                mount_point, e
            );
            umount_dir(root).with_context(|| format!("failed to revert {root}"))?;
            bail!(e);
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn mount_ext4(_src: &str, _target: &str, _autodrop: bool) -> Result<()> {
    unimplemented!()
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn umount_dir(_src: &str) -> Result<()> {
    unimplemented!()
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn mount_overlay(
    _root: &String,
    _module_roots: &Vec<String>,
    _workdir: Option<PathBuf>,
    _upperdir: Option<PathBuf>,
) -> Result<()> {
    unimplemented!()
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn mount_tmpfs(_dest: impl AsRef<Path>) -> Result<()> {
    unimplemented!()
}
