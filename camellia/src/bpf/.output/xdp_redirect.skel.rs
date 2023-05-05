// SPDX-License-Identifier: (LGPL-2.1 OR BSD-2-Clause)
//
// THIS FILE IS AUTOGENERATED BY CARGO-LIBBPF-GEN!

pub use self::imp::*;

#[allow(dead_code)]
#[allow(non_snake_case)]
#[allow(non_camel_case_types)]
#[allow(clippy::transmute_ptr_to_ref)]
#[allow(clippy::upper_case_acronyms)]
mod imp {
    use libbpf_rs::libbpf_sys;

    fn build_skel_config() -> libbpf_rs::Result<libbpf_rs::skeleton::ObjectSkeletonConfig<'static>>
    {
        let mut builder = libbpf_rs::skeleton::ObjectSkeletonConfigBuilder::new(DATA);
        builder.name("xdp_redirect_bpf").prog("xdp_redirect");

        builder.build()
    }

    #[derive(Default)]
    pub struct XdpRedirectSkelBuilder {
        pub obj_builder: libbpf_rs::ObjectBuilder,
    }

    impl<'a> XdpRedirectSkelBuilder {
        pub fn open(mut self) -> libbpf_rs::Result<OpenXdpRedirectSkel<'a>> {
            let mut skel_config = build_skel_config()?;
            let open_opts = self.obj_builder.opts(std::ptr::null());

            let ret =
                unsafe { libbpf_sys::bpf_object__open_skeleton(skel_config.get(), &open_opts) };
            if ret != 0 {
                return Err(libbpf_rs::Error::System(-ret));
            }

            let obj = unsafe { libbpf_rs::OpenObject::from_ptr(skel_config.object_ptr())? };

            Ok(OpenXdpRedirectSkel { obj, skel_config })
        }

        pub fn open_opts(
            self,
            open_opts: libbpf_sys::bpf_object_open_opts,
        ) -> libbpf_rs::Result<OpenXdpRedirectSkel<'a>> {
            let mut skel_config = build_skel_config()?;

            let ret =
                unsafe { libbpf_sys::bpf_object__open_skeleton(skel_config.get(), &open_opts) };
            if ret != 0 {
                return Err(libbpf_rs::Error::System(-ret));
            }

            let obj = unsafe { libbpf_rs::OpenObject::from_ptr(skel_config.object_ptr())? };

            Ok(OpenXdpRedirectSkel { obj, skel_config })
        }
    }

    pub struct OpenXdpRedirectProgs<'a> {
        inner: &'a libbpf_rs::OpenObject,
    }

    impl<'a> OpenXdpRedirectProgs<'a> {
        pub fn xdp_redirect(&self) -> &libbpf_rs::OpenProgram {
            self.inner.prog("xdp_redirect").unwrap()
        }
    }

    pub struct OpenXdpRedirectProgsMut<'a> {
        inner: &'a mut libbpf_rs::OpenObject,
    }

    impl<'a> OpenXdpRedirectProgsMut<'a> {
        pub fn xdp_redirect(&mut self) -> &mut libbpf_rs::OpenProgram {
            self.inner.prog_mut("xdp_redirect").unwrap()
        }
    }

    pub struct OpenXdpRedirectSkel<'a> {
        pub obj: libbpf_rs::OpenObject,
        skel_config: libbpf_rs::skeleton::ObjectSkeletonConfig<'a>,
    }

    impl<'a> OpenXdpRedirectSkel<'a> {
        pub fn load(mut self) -> libbpf_rs::Result<XdpRedirectSkel<'a>> {
            let ret = unsafe { libbpf_sys::bpf_object__load_skeleton(self.skel_config.get()) };
            if ret != 0 {
                return Err(libbpf_rs::Error::System(-ret));
            }

            let obj = unsafe { libbpf_rs::Object::from_ptr(self.obj.take_ptr())? };

            Ok(XdpRedirectSkel {
                obj,
                skel_config: self.skel_config,
                links: XdpRedirectLinks::default(),
            })
        }

        pub fn progs(&self) -> OpenXdpRedirectProgs {
            OpenXdpRedirectProgs { inner: &self.obj }
        }

        pub fn progs_mut(&mut self) -> OpenXdpRedirectProgsMut {
            OpenXdpRedirectProgsMut {
                inner: &mut self.obj,
            }
        }
    }

    pub struct XdpRedirectProgs<'a> {
        inner: &'a libbpf_rs::Object,
    }

    impl<'a> XdpRedirectProgs<'a> {
        pub fn xdp_redirect(&self) -> &libbpf_rs::Program {
            self.inner.prog("xdp_redirect").unwrap()
        }
    }

    pub struct XdpRedirectProgsMut<'a> {
        inner: &'a mut libbpf_rs::Object,
    }

    impl<'a> XdpRedirectProgsMut<'a> {
        pub fn xdp_redirect(&mut self) -> &mut libbpf_rs::Program {
            self.inner.prog_mut("xdp_redirect").unwrap()
        }
    }

    #[derive(Default)]
    pub struct XdpRedirectLinks {
        pub xdp_redirect: Option<libbpf_rs::Link>,
    }

    pub struct XdpRedirectSkel<'a> {
        pub obj: libbpf_rs::Object,
        skel_config: libbpf_rs::skeleton::ObjectSkeletonConfig<'a>,
        pub links: XdpRedirectLinks,
    }

    unsafe impl<'a> Send for XdpRedirectSkel<'a> {}
    unsafe impl<'a> Sync for XdpRedirectSkel<'a> {}

    impl<'a> XdpRedirectSkel<'a> {
        pub fn progs(&self) -> XdpRedirectProgs {
            XdpRedirectProgs { inner: &self.obj }
        }

        pub fn progs_mut(&mut self) -> XdpRedirectProgsMut {
            XdpRedirectProgsMut {
                inner: &mut self.obj,
            }
        }

        pub fn attach(&mut self) -> libbpf_rs::Result<()> {
            let ret = unsafe { libbpf_sys::bpf_object__attach_skeleton(self.skel_config.get()) };
            if ret != 0 {
                return Err(libbpf_rs::Error::System(-ret));
            }

            self.links = XdpRedirectLinks {
                xdp_redirect: (|| {
                    Ok(core::ptr::NonNull::new(self.skel_config.prog_link_ptr(0)?)
                        .map(|ptr| unsafe { libbpf_rs::Link::from_ptr(ptr) }))
                })()?,
            };

            Ok(())
        }
    }

    const DATA: &[u8] = &[
        127, 69, 76, 70, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 247, 0, 1, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 152, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0,
        0, 64, 0, 7, 0, 1, 0, 0, 46, 115, 116, 114, 116, 97, 98, 0, 46, 115, 121, 109, 116, 97, 98,
        0, 120, 100, 112, 0, 108, 105, 99, 101, 110, 115, 101, 0, 120, 100, 112, 95, 114, 101, 100,
        105, 114, 101, 99, 116, 46, 98, 112, 102, 46, 99, 0, 120, 100, 112, 95, 114, 101, 100, 105,
        114, 101, 99, 116, 0, 95, 95, 108, 105, 99, 101, 110, 115, 101, 0, 46, 66, 84, 70, 0, 46,
        66, 84, 70, 46, 101, 120, 116, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 29, 0, 0, 0, 4, 0, 241, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 3, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 48, 0, 0,
        0, 18, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 61, 0, 0, 0, 17, 0, 4, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 183, 0, 0, 0, 4, 0, 0, 0, 149, 0, 0, 0, 0,
        0, 0, 0, 71, 80, 76, 0, 0, 0, 0, 0, 159, 235, 1, 0, 24, 0, 0, 0, 0, 0, 0, 0, 12, 1, 0, 0,
        12, 1, 0, 0, 246, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 0, 0, 0, 1, 0, 0, 0, 6, 0, 0, 4, 24,
        0, 0, 0, 8, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0, 0, 3, 0, 0, 0, 32, 0, 0, 0, 22, 0,
        0, 0, 3, 0, 0, 0, 64, 0, 0, 0, 32, 0, 0, 0, 3, 0, 0, 0, 96, 0, 0, 0, 48, 0, 0, 0, 3, 0, 0,
        0, 128, 0, 0, 0, 63, 0, 0, 0, 3, 0, 0, 0, 160, 0, 0, 0, 78, 0, 0, 0, 0, 0, 0, 8, 4, 0, 0,
        0, 84, 0, 0, 0, 0, 0, 0, 1, 4, 0, 0, 0, 32, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 13, 6, 0, 0, 0,
        97, 0, 0, 0, 1, 0, 0, 0, 101, 0, 0, 0, 0, 0, 0, 1, 4, 0, 0, 0, 32, 0, 0, 1, 105, 0, 0, 0,
        1, 0, 0, 12, 5, 0, 0, 0, 118, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 8, 0, 0, 1, 0, 0, 0, 0, 0,
        0, 0, 3, 0, 0, 0, 0, 8, 0, 0, 0, 10, 0, 0, 0, 4, 0, 0, 0, 123, 0, 0, 0, 0, 0, 0, 1, 4, 0,
        0, 0, 32, 0, 0, 0, 143, 0, 0, 0, 0, 0, 0, 14, 9, 0, 0, 0, 1, 0, 0, 0, 234, 0, 0, 0, 1, 0,
        0, 15, 4, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 120, 100, 112, 95, 109, 100, 0,
        100, 97, 116, 97, 0, 100, 97, 116, 97, 95, 101, 110, 100, 0, 100, 97, 116, 97, 95, 109,
        101, 116, 97, 0, 105, 110, 103, 114, 101, 115, 115, 95, 105, 102, 105, 110, 100, 101, 120,
        0, 114, 120, 95, 113, 117, 101, 117, 101, 95, 105, 110, 100, 101, 120, 0, 101, 103, 114,
        101, 115, 115, 95, 105, 102, 105, 110, 100, 101, 120, 0, 95, 95, 117, 51, 50, 0, 117, 110,
        115, 105, 103, 110, 101, 100, 32, 105, 110, 116, 0, 99, 116, 120, 0, 105, 110, 116, 0, 120,
        100, 112, 95, 114, 101, 100, 105, 114, 101, 99, 116, 0, 99, 104, 97, 114, 0, 95, 95, 65,
        82, 82, 65, 89, 95, 83, 73, 90, 69, 95, 84, 89, 80, 69, 95, 95, 0, 95, 95, 108, 105, 99,
        101, 110, 115, 101, 0, 47, 104, 111, 109, 101, 47, 109, 105, 110, 104, 117, 47, 99, 97,
        109, 101, 108, 108, 105, 97, 47, 99, 97, 109, 101, 108, 108, 105, 97, 47, 46, 47, 115, 114,
        99, 47, 98, 112, 102, 47, 120, 100, 112, 95, 114, 101, 100, 105, 114, 101, 99, 116, 46, 98,
        112, 102, 46, 99, 0, 9, 114, 101, 116, 117, 114, 110, 32, 88, 68, 80, 95, 82, 69, 68, 73,
        82, 69, 67, 84, 59, 0, 108, 105, 99, 101, 110, 115, 101, 0, 120, 100, 112, 0, 0, 0, 0, 0,
        0, 0, 159, 235, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0, 20, 0, 0, 0, 28, 0, 0, 0, 48,
        0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 242, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 16, 0,
        0, 0, 242, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 153, 0, 0, 0, 212, 0, 0, 0, 2, 28, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0,
        0, 0, 0, 0, 85, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        152, 0, 0, 0, 0, 0, 0, 0, 120, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0,
        0, 0, 0, 24, 0, 0, 0, 0, 0, 0, 0, 17, 0, 0, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 16, 1, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 21, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 1, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 71, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 40, 1, 0, 0, 0, 0, 0, 0, 26, 2, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 76, 0, 0, 0, 1, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 72, 3, 0, 0, 0, 0, 0, 0, 80, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
}
