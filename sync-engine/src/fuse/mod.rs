pub mod fuse_backend;

use fuse3::raw::Session;
use fuse3::MountOptions;
use std::path::Path;

pub async fn mount_fuse(
    fs: fuse_backend::TwakeFuseFs,
    mount_point: &str,
) -> Result<fuse3::raw::MountHandle, Box<dyn std::error::Error>> {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };

    let mut mount_options = MountOptions::default();
    mount_options.uid(uid).gid(gid).fs_name("twake");

    let mount_handle = Session::new(mount_options)
        .mount_with_unprivileged(fs, mount_point)
        .await?;

    Ok(mount_handle)
}
