use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{thread, time};

#[path = "bpf/.output/xdp_redirect.skel.rs"]
mod xdp_redirect;
use xdp_redirect::*;
use anyhow::Result;


pub fn load_xdp(ifindex: i32) -> Result<()> {
    let skel_builder = XdpRedirectSkelBuilder::default();
    let mut skel = skel_builder.open()?.load()?;
    let link = skel.progs_mut().xdp_redirect().attach_xdp(ifindex)?;
    skel.links = XdpRedirectLinks {
        xdp_redirect: Some(link)
    };

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    while running.load(Ordering::SeqCst) {
        eprint!(".");
        thread::sleep(time::Duration::from_secs(1));
    }

    Ok(())
}