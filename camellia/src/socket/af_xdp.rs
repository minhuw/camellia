use std::cell::RefCell;
use std::cmp::min;
use std::ffi::CString;
use std::os::fd::AsRawFd;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;

use libbpf_rs::libbpf_sys;
use libc::sendto;
use libc::MSG_DONTWAIT;
use libxdp_sys::xsk_socket;
use libxdp_sys::xsk_socket__create_shared;
use libxdp_sys::xsk_socket_config;
use libxdp_sys::xsk_socket_config__bindgen_ty_1;
use libxdp_sys::XSK_RING_CONS__DEFAULT_NUM_DESCS;
use libxdp_sys::XSK_RING_PROD__DEFAULT_NUM_DESCS;
use libxdp_sys::{xsk_ring_cons, xsk_ring_prod};
use libxdp_sys::{
    xsk_ring_cons__peek, xsk_ring_cons__release, xsk_ring_cons__rx_desc,
    xsk_ring_prod__needs_wakeup, xsk_ring_prod__reserve, xsk_ring_prod__submit,
    xsk_ring_prod__tx_desc, xsk_socket__create, xsk_socket__delete, xsk_socket__fd,
};
use nix::errno::Errno;

use crate::error::CamelliaError;
use crate::umem::base::CompletionQueue;
use crate::umem::base::DedicatedAccessor;
use crate::umem::base::FillQueue;
use crate::umem::base::UMem;
use crate::umem::frame::AppFrame;
use crate::umem::frame::RxFrame;
use crate::umem::frame::TxFrame;
use crate::umem::shared::SharedAccessor;
use crate::umem::UMemAccessor;

#[derive(Debug)]
pub struct RxQueue {
    inner: xsk_ring_cons,
}

impl  Default for RxQueue {
    fn default() -> Self {
        Self {
            inner: xsk_ring_cons {
                cached_prod: 0,
                cached_cons: 0,
                mask: 0,
                size: 0,
                producer: std::ptr::null_mut(),
                consumer: std::ptr::null_mut(),
                ring: std::ptr::null_mut(),
                flags: std::ptr::null_mut()
            }
        }
    }
}

#[derive(Debug)]
pub struct TxQueue {
    inner: xsk_ring_prod,
}

impl Default for TxQueue {
    fn default() -> Self {
        Self {
            inner: xsk_ring_prod {
                cached_prod: 0,
                cached_cons: 0,
                mask: 0,
                size: 0,
                producer: std::ptr::null_mut(),
                consumer: std::ptr::null_mut(),
                ring: std::ptr::null_mut(),
                flags: std::ptr::null_mut()
            }
        }
    }
}

pub struct TxDescriptor {}

pub enum XDPMode {
    Generic,
    Driver,
    Hardware,
}

pub enum XSKUMem {
    Dedicated(UMem),
    Shared(Arc<Mutex<UMem>>),
}

pub struct XskSocketBuilder<M>
where
    M: UMemAccessor,
{
    ifname: Option<String>,
    queue_index: Option<u32>,
    rx_queue_size: u32,
    tx_queue_size: u32,
    no_default_prog: bool,
    zero_copy: bool,
    cooperate_schedule: bool,
    mode: XDPMode,
    umem: Option<M::UMemRef>,
}

impl<M> Default for XskSocketBuilder<M>
where
    M: UMemAccessor,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<M> XskSocketBuilder<M>
where
    M: UMemAccessor,
{
    pub fn new() -> Self {
        Self {
            ifname: None,
            queue_index: None,
            rx_queue_size: XSK_RING_CONS__DEFAULT_NUM_DESCS,
            tx_queue_size: XSK_RING_PROD__DEFAULT_NUM_DESCS,
            mode: XDPMode::Driver,
            umem: None,
            no_default_prog: false,
            zero_copy: false,
            cooperate_schedule: false,
        }
    }

    fn construct_config(&self) -> Result<xsk_socket_config, CamelliaError> {
        if self.umem.is_none() {
            return Err(CamelliaError::InvalidArgument(
                "UMem is not set".to_string(),
            ));
        }

        if self.ifname.is_none() {
            return Err(CamelliaError::InvalidArgument(
                "Interface name is not set".to_string(),
            ));
        }

        if self.queue_index.is_none() {
            return Err(CamelliaError::InvalidArgument(
                "Queue index is not set".to_string(),
            ));
        }

        let libxdp_flags = if self.no_default_prog {
            libxdp_sys::XSK_LIBXDP_FLAGS__INHIBIT_PROG_LOAD
        } else {
            0
        };

        let xdp_flags = match self.mode {
            XDPMode::Generic => libbpf_sys::XDP_FLAGS_SKB_MODE,
            XDPMode::Driver => libbpf_sys::XDP_FLAGS_DRV_MODE,
            XDPMode::Hardware => libbpf_sys::XDP_FLAGS_HW_MODE,
        };

        let bind_flags = match self.zero_copy {
            true => libxdp_sys::XDP_ZEROCOPY,
            false => 0,
        } | match self.cooperate_schedule {
            true => libxdp_sys::XDP_USE_NEED_WAKEUP,
            false => 0,
        };

        Ok(xsk_socket_config {
            rx_size: self.rx_queue_size,
            tx_size: self.tx_queue_size,
            __bindgen_anon_1: xsk_socket_config__bindgen_ty_1 { libxdp_flags },
            bind_flags: bind_flags as u16,
            xdp_flags,
        })
    }

    pub fn ifname(mut self, ifname: &str) -> Self {
        self.ifname = Some(ifname.to_string());
        self
    }

    pub fn queue_index(mut self, queue_index: u32) -> Self {
        self.queue_index = Some(queue_index);
        self
    }

    pub fn rx_queue_size(mut self, rx_queue_size: u32) -> Self {
        self.rx_queue_size = rx_queue_size;
        self
    }

    pub fn tx_queue_size(mut self, tx_queue_size: u32) -> Self {
        self.tx_queue_size = tx_queue_size;
        self
    }

    pub fn no_default_prog(mut self) -> Self {
        self.no_default_prog = true;
        self
    }

    pub fn xdp_mode(mut self, mode: XDPMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn enable_zero_copy(mut self) -> Self {
        self.zero_copy = true;
        self
    }

    pub fn enable_cooperate_schedule(mut self) -> Self {
        self.cooperate_schedule = true;
        self
    }

    pub fn with_umem(mut self, umem: M::UMemRef) -> Self {
        if self.umem.is_some() {
            panic!("UMem is already set");
        }
        self.umem = Some(umem);
        self
    }
}

impl XskSocketBuilder<DedicatedAccessor> {
    pub fn build(self) -> Result<XskSocket<DedicatedAccessor>, CamelliaError> {
        let config = self.construct_config()?;
        XskSocket::<DedicatedAccessor>::new(
            &self.ifname.unwrap(),
            self.queue_index.unwrap(),
            self.umem.unwrap(),
            config,
        )
    }
}

impl XskSocketBuilder<SharedAccessor> {
    pub fn build_shared(self) -> Result<XskSocket<SharedAccessor>, CamelliaError> {
        let config = self.construct_config()?;
        XskSocket::<SharedAccessor>::new(
            &self.ifname.unwrap(),
            self.queue_index.unwrap(),
            self.umem.unwrap(),
            config,
        )
    }
}

pub struct XskSocket<M: UMemAccessor> {
    inner: *mut xsk_socket,
    umem: M::AccessorRef,
    rx: Pin<Box<RxQueue>>,
    tx: Pin<Box<TxQueue>>,
}

impl XskSocket<SharedAccessor> {
    pub fn new(
        ifname: &str,
        queue_index: u32,
        umem: <SharedAccessor as UMemAccessor>::UMemRef,
        config: xsk_socket_config,
    ) -> Result<Self, CamelliaError> {
        let mut raw_socket: *mut xsk_socket = std::ptr::null_mut();
        let mut rx_queue = Box::pin(RxQueue::default());
        let mut tx_queue = Box::pin(TxQueue::default());
        let mut fill_queue = Box::pin(FillQueue::default());
        let mut completion_queue = Box::pin(CompletionQueue::default());

        let ifname = CString::new(ifname).unwrap();
        log::info!(
            "create AF_XDP socket on device {:?} (queue {})",
            ifname,
            queue_index
        );

        unsafe {
            match xsk_socket__create_shared(
                &mut raw_socket,
                ifname.as_ptr(),
                queue_index,
                umem.lock().unwrap().inner(),
                &mut rx_queue.inner,
                &mut tx_queue.inner,
                &mut fill_queue.0,
                &mut completion_queue.0,
                &config,
            ) {
                0 => {}
                errno => {
                    return Err(Errno::from_i32(-errno).into());
                }
            }
        }

        let umem = Rc::new(RefCell::new(SharedAccessor::new(
            umem,
            fill_queue,
            completion_queue,
        )?));

        // TODO: validate that the RX ring is fulfilled
        <SharedAccessor as UMemAccessor>::fill(&umem, config.rx_size as usize).unwrap();

        Ok(XskSocket {
            inner: raw_socket,
            umem,
            rx: rx_queue,
            tx: tx_queue,
        })
    }
}

impl XskSocket<DedicatedAccessor> {
    pub fn new(
        ifname: &str,
        queue_index: u32,
        umem: <DedicatedAccessor as UMemAccessor>::UMemRef,
        config: xsk_socket_config,
    ) -> Result<Self, CamelliaError> {
        let mut raw_socket: *mut xsk_socket = std::ptr::null_mut();
        let mut rx_queue = Box::pin(RxQueue::default());
        let mut tx_queue = Box::pin(TxQueue::default());

        let ifname = CString::new(ifname).unwrap();
        log::info!(
            "create AF_XDP socket on device {:?} (queue {})",
            ifname,
            queue_index
        );

        unsafe {
            match xsk_socket__create(
                &mut raw_socket,
                ifname.as_ptr(),
                queue_index,
                umem.inner() as *mut _,
                &mut rx_queue.inner,
                &mut tx_queue.inner,
                &config,
            ) {
                0 => {}
                errno => {
                    return Err(Errno::from_i32(-errno).into());
                }
            }
        }

        let umem = umem.into();
        // TODO: validate that the RX ring is fulfilled
        <DedicatedAccessor as UMemAccessor>::fill(&umem, config.rx_size as usize).unwrap();

        Ok(XskSocket {
            inner: raw_socket,
            umem,
            rx: rx_queue,
            tx: tx_queue,
        })
    }
}

impl<M> XskSocket<M>
where
    M: UMemAccessor,
{
    pub fn recv(&mut self) -> Result<Option<RxFrame<M>>, CamelliaError> {
        let mut received = self.recv_bulk(1)?;
        assert!(received.len() <= 1);
        Ok(received.pop())
    }

    pub fn recv_bulk(&mut self, size: usize) -> Result<Vec<RxFrame<M>>, CamelliaError> {
        let mut start_index = 0;

        let received: u32 =
            unsafe { xsk_ring_cons__peek(&mut self.rx.inner, size as u32, &mut start_index) };

        assert!((received as usize) <= size);

        let frames = (0..received as usize)
            .map(|i| {
                let (addr, len) = unsafe {
                    let rx_desp = xsk_ring_cons__rx_desc(&self.rx.inner, start_index + i as u32);
                    ((*rx_desp).addr, (*rx_desp).len)
                };

                let chunk = M::extract_recv(&self.umem, addr);
                RxFrame::from_chunk(chunk, self.umem.clone(), addr as usize, len as usize)
            })
            .collect();

        unsafe {
            xsk_ring_cons__release(&mut self.rx.inner, received);
        }

        // TODO: add an option controlling whether to fill the umem eagerly
        let filled = M::fill(&self.umem, received as usize)?;

        if filled < (received as usize) {
            log::warn!("fill failed, filled: {}, received: {}", filled, received);
        }

        Ok(frames)
    }

    pub fn allocate(&mut self, n: usize) -> Result<Vec<AppFrame<M>>, CamelliaError> {
        UMemAccessor::allocate(&self.umem, n)
    }

    pub fn send<T>(&mut self, frame: T) -> Result<Option<T>, CamelliaError>
    where
        T: Into<TxFrame<M>>,
    {
        let mut remaining = self.send_bulk([frame])?;
        assert!(remaining.len() <= 1);

        if remaining.len() == 1 {
            Ok(Some(remaining.pop().unwrap()))
        } else {
            Ok(None)
        }
    }

    pub fn send_bulk<Iter, T>(&mut self, frames: Iter) -> Result<Vec<T>, CamelliaError>
    where
        T: Into<TxFrame<M>>,
        Iter: IntoIterator<Item = T>,
        Iter::IntoIter: ExactSizeIterator,
    {
        let mut start_index = 0;
        let mut remaining = Vec::new();

        M::recycle(&self.umem)?;

        let iter = frames.into_iter();

        let reserved_desp = unsafe {
            xsk_ring_prod__reserve(&mut self.tx.inner, iter.len() as u32, &mut start_index)
        };

        let actual_sent = min(reserved_desp, iter.len() as u32);

        for (send_index, frame) in iter.enumerate() {
            if (send_index as u32) < actual_sent {
                let frame: TxFrame<M> = frame.into();

                if !M::equal(frame.umem(), &self.umem) {
                    return Err(CamelliaError::InvalidArgument(
                        "Frame does not belong to this socket".to_string(),
                    ));
                }

                unsafe {
                    let tx_desc = xsk_ring_prod__tx_desc(
                        &mut self.tx.inner,
                        start_index + (send_index as u32),
                    );
                    (*tx_desc).addr = frame.xdp_address() as u64;
                    (*tx_desc).len = frame.len() as u32;
                    (*tx_desc).options = 0;
                };

                M::register_send(&self.umem, frame.take())
            } else {
                remaining.push(frame);
            }
        }

        unsafe {
            xsk_ring_prod__submit(&mut self.tx.inner, actual_sent);

            // When cooperate schedule is disabled, we always need to wake up the TX queue
            // https://lore.kernel.org/bpf/20201130185205.196029-5-bjorn.topel@gmail.com/
            // But the wakeup flag is set even when XDP_USE_NEED_WAKEUP is not set, so we
            // happlily always checks the XDP_RING_NEED_WAKEUP flag.
            if xsk_ring_prod__needs_wakeup(&self.tx.inner) != 0 {
                Errno::result(sendto(
                    self.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    MSG_DONTWAIT,
                    std::ptr::null(),
                    0,
                ))?;
            }
        }

        Ok(remaining)
    }
}

impl<M> Drop for XskSocket<M>
where
    M: UMemAccessor,
{
    fn drop(&mut self) {
        unsafe { xsk_socket__delete(self.inner) }
    }
}

impl<M> AsRawFd for XskSocket<M>
where
    M: UMemAccessor,
{
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        unsafe { xsk_socket__fd(self.inner) }
    }
}
