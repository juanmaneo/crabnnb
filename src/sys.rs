use std::io;

pub type RawFd = i32;

pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EWOULDBLOCK: i32 = EAGAIN;
pub const EACCES: i32 = 13;
pub const ENOSYS: i32 = 38;
pub const EAFNOSUPPORT: i32 = 97;
pub const ECONNREFUSED: i32 = 111;

#[cfg(target_arch = "x86_64")]
mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const CLOSE: u64 = 3;
    pub const IOCTL: u64 = 16;
    pub const SOCKET: u64 = 41;
    pub const CONNECT: u64 = 42;
    pub const BIND: u64 = 49;
    pub const LISTEN: u64 = 50;
    pub const SOCKETPAIR: u64 = 53;
    pub const SETSOCKOPT: u64 = 54;
    pub const ACCEPT4: u64 = 288;
}

#[cfg(target_arch = "aarch64")]
mod nr {
    pub const READ: u64 = 63;
    pub const WRITE: u64 = 64;
    pub const CLOSE: u64 = 57;
    pub const IOCTL: u64 = 29;
    pub const SOCKET: u64 = 198;
    pub const CONNECT: u64 = 203;
    pub const BIND: u64 = 200;
    pub const LISTEN: u64 = 201;
    pub const SOCKETPAIR: u64 = 199;
    pub const SETSOCKOPT: u64 = 208;
    pub const ACCEPT4: u64 = 242;
}

#[cfg(target_arch = "x86_64")]
unsafe fn raw_syscall(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    ret
}

#[cfg(target_arch = "aarch64")]
unsafe fn raw_syscall(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "svc 0",
            in("x8") nr,
            inlateout("x0") a1 => ret,
            in("x1") a2,
            in("x2") a3,
            in("x3") a4,
            in("x4") a5,
            in("x5") a6,
            options(nostack)
        );
    }
    ret
}

fn cvt(ret: i64) -> io::Result<i64> {
    if (ret as u64) >= 0xFFFF_FFFF_FFFF_F001 {
        Err(io::Error::from_raw_os_error((-ret) as i32))
    } else {
        Ok(ret)
    }
}

pub trait Sys {
    fn socket(&self, domain: i32, ty: i32, protocol: i32) -> io::Result<RawFd>;
    fn socketpair(&self, domain: i32, ty: i32, protocol: i32, fds: &mut [RawFd; 2])
        -> io::Result<()>;
    fn bind(&self, fd: RawFd, addr: &[u8]) -> io::Result<()>;
    fn listen(&self, fd: RawFd, backlog: i32) -> io::Result<()>;
    fn connect(&self, fd: RawFd, addr: &[u8]) -> io::Result<()>;
    fn accept(&self, fd: RawFd) -> io::Result<RawFd>;
    fn read(&self, fd: RawFd, buf: &mut [u8]) -> io::Result<usize>;
    fn write(&self, fd: RawFd, buf: &[u8]) -> io::Result<usize>;
    fn close(&self, fd: RawFd) -> io::Result<()>;
    fn ioctl(&self, fd: RawFd, request: u64, arg: &mut [u8]) -> io::Result<usize>;
    fn setsockopt(&self, fd: RawFd, level: i32, optname: i32, optval: &[u8]) -> io::Result<()>;
}

pub struct LinuxSys;

impl Sys for LinuxSys {
    fn socket(&self, domain: i32, ty: i32, protocol: i32) -> io::Result<RawFd> {
        let ret = unsafe {
            raw_syscall(
                nr::SOCKET,
                domain as u64,
                ty as u64,
                protocol as u64,
                0,
                0,
                0,
            )
        };
        cvt(ret).map(|r| r as RawFd)
    }

    fn socketpair(
        &self,
        domain: i32,
        ty: i32,
        protocol: i32,
        fds: &mut [RawFd; 2],
    ) -> io::Result<()> {
        let mut raw = [0 as RawFd; 2];
        let ret = unsafe {
            raw_syscall(
                nr::SOCKETPAIR,
                domain as u64,
                ty as u64,
                protocol as u64,
                raw.as_mut_ptr() as u64,
                0,
                0,
            )
        };
        cvt(ret)?;
        *fds = raw;
        Ok(())
    }

    fn bind(&self, fd: RawFd, addr: &[u8]) -> io::Result<()> {
        let ret = unsafe {
            raw_syscall(
                nr::BIND,
                fd as u64,
                addr.as_ptr() as u64,
                addr.len() as u64,
                0,
                0,
                0,
            )
        };
        cvt(ret)?;
        Ok(())
    }

    fn listen(&self, fd: RawFd, backlog: i32) -> io::Result<()> {
        let ret = unsafe { raw_syscall(nr::LISTEN, fd as u64, backlog as u64, 0, 0, 0, 0) };
        cvt(ret)?;
        Ok(())
    }

    fn connect(&self, fd: RawFd, addr: &[u8]) -> io::Result<()> {
        let ret = unsafe {
            raw_syscall(
                nr::CONNECT,
                fd as u64,
                addr.as_ptr() as u64,
                addr.len() as u64,
                0,
                0,
                0,
            )
        };
        cvt(ret)?;
        Ok(())
    }

    fn accept(&self, fd: RawFd) -> io::Result<RawFd> {
        let ret = unsafe { raw_syscall(nr::ACCEPT4, fd as u64, 0, 0, 0, 0, 0) };
        cvt(ret).map(|r| r as RawFd)
    }

    fn read(&self, fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
        let ret = unsafe {
            raw_syscall(
                nr::READ,
                fd as u64,
                buf.as_mut_ptr() as u64,
                buf.len() as u64,
                0,
                0,
                0,
            )
        };
        cvt(ret).map(|r| r as usize)
    }

    fn write(&self, fd: RawFd, buf: &[u8]) -> io::Result<usize> {
        let ret = unsafe {
            raw_syscall(
                nr::WRITE,
                fd as u64,
                buf.as_ptr() as u64,
                buf.len() as u64,
                0,
                0,
                0,
            )
        };
        cvt(ret).map(|r| r as usize)
    }

    fn close(&self, fd: RawFd) -> io::Result<()> {
        let ret = unsafe { raw_syscall(nr::CLOSE, fd as u64, 0, 0, 0, 0, 0) };
        cvt(ret)?;
        Ok(())
    }

    fn ioctl(&self, fd: RawFd, request: u64, arg: &mut [u8]) -> io::Result<usize> {
        let ret = unsafe {
            raw_syscall(
                nr::IOCTL,
                fd as u64,
                request,
                arg.as_mut_ptr() as u64,
                0,
                0,
                0,
            )
        };
        cvt(ret)?;
        Ok(arg.len())
    }

    fn setsockopt(&self, fd: RawFd, level: i32, optname: i32, optval: &[u8]) -> io::Result<()> {
        let ret = unsafe {
            raw_syscall(
                nr::SETSOCKOPT,
                fd as u64,
                level as u64,
                optname as u64,
                optval.as_ptr() as u64,
                optval.len() as u64,
                0,
            )
        };
        cvt(ret)?;
        Ok(())
    }
}
