//! Components for interacting with IPv6 messages on GNRC

use core::mem::MaybeUninit;

use riot_sys::{ipv6_addr_from_str, ipv6_addr_t, kernel_pid_t};

use crate::error::{NegativeErrorExt, NumericError};

impl super::Netif {
    pub fn ipv6_addrs(
        &self,
    ) -> Result<IPv6AddrList<{ riot_sys::CONFIG_GNRC_NETIF_IPV6_ADDRS_NUMOF as _ }>, NumericError>
    {
        let mut addrs = IPv6AddrList {
            // unsafe: as per "Initializing an array element-by-element" documentation
            addresses: unsafe { MaybeUninit::uninit().assume_init() },
            len: 0,
        };
        let result = unsafe {
            riot_sys::gnrc_netif_ipv6_addrs_get(
                crate::inline_cast(self.0),
                addrs.addresses.as_mut() as *mut _ as _, /* justified by array guarantees and repr(Transparent) */
                core::mem::size_of_val(&addrs.addresses) as _,
            )
        };
        addrs.len = (result.negative_to_error()? as usize) / core::mem::size_of::<IPv6Addr>();
        Ok(addrs)
    }
}

/// Helper for [Netif::ipv6_addrs]: As the [riot_sys::gnrc_netif_ipv6_addrs_get] function requires
/// a multiple-address buffer to write in, this carries a suitable buffer.
pub struct IPv6AddrList<const MAX: usize> {
    pub(super) addresses: [MaybeUninit<IPv6Addr>; MAX],
    pub(super) len: usize,
}

impl<const MAX: usize> IPv6AddrList<MAX> {
    #[deprecated(note = "&IPv6AddrList now implements IntoIterator")]
    pub fn addresses(&self) -> &[IPv6Addr] {
        self
    }
}

impl<const MAX: usize> core::ops::Deref for IPv6AddrList<MAX> {
    type Target = [IPv6Addr];

    fn deref(&self) -> &[IPv6Addr] {
        let slice = &self.addresses[..self.len];
        // unsafe: as per "Initializing an array element-by-element" documentation
        unsafe { core::mem::transmute(slice) }
    }
}

impl<'a, const MAX: usize> core::iter::IntoIterator for &'a IPv6AddrList<MAX> {
    type Item = &'a IPv6Addr;

    type IntoIter = core::slice::Iter<'a, IPv6Addr>;

    fn into_iter(self) -> Self::IntoIter {
        self[..].iter()
    }
}

#[repr(transparent)] // which allows the IPv6AddrList addresss to be passed to gnrc_netif_ipv6_addrs_get
pub struct IPv6Addr {
    inner: ipv6_addr_t,
}

// When no_std_net / embedded_nal is present, it may be a good idea to run through there (or allow
// configuration to optimize which route to take for best deduplication of code)
impl ::core::str::FromStr for IPv6Addr {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // It'd be nice to use std::net::IPv6Addr::from_str, but the parser is generic over
        // families (maybe at some point we'll need that here too, but not now), and it's in std
        // rather then core for reasons I can't really follow.

        let s = s.as_bytes();

        let mut with_null = [0u8; 32 + 7 + 1]; // 32 nibbles + 7 colons + null byte
        if s.len() > with_null.len() - 1 {
            // Obviously too long to be a valid plain address
            return Err(());
        }
        with_null[..s.len()].copy_from_slice(s);

        let mut inner: MaybeUninit<ipv6_addr_t> = MaybeUninit::uninit();

        let conversion_result = unsafe {
            ipv6_addr_from_str(
                inner.as_mut_ptr(),
                cstr_core::CStr::from_bytes_with_nul_unchecked(&with_null).as_ptr(),
            )
        };

        match conversion_result as usize {
            0 => Err(()),
            _ => Ok(Self {
                inner: unsafe { inner.assume_init() },
            }),
        }
    }
}

// When no_std_net / embedded_nal is present, it may be a good idea to run through there.
impl ::core::fmt::Debug for IPv6Addr {
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        let as_u8 = unsafe { &self.inner.u8_ };
        write!(
            f,
            "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:\
             {:02x}{:02x}:{:02x}{:02x}",
            as_u8[0],
            as_u8[1],
            as_u8[2],
            as_u8[3],
            as_u8[4],
            as_u8[5],
            as_u8[6],
            as_u8[7],
            as_u8[8],
            as_u8[9],
            as_u8[10],
            as_u8[11],
            as_u8[12],
            as_u8[13],
            as_u8[14],
            as_u8[15],
        )
    }
}

impl IPv6Addr {
    pub fn raw(&self) -> &[u8; 16] {
        unsafe { &self.inner.u8_ }
    }

    pub unsafe fn as_ptr(&self) -> *const ipv6_addr_t {
        &self.inner
    }

    /// Given a ipv6_addr_t, copy the data out into an IPv6Addr.
    ///
    /// That might be inefficient in many cases, and there might be a way to get an &IPv6Addr
    /// newtyped from a &ipv6_addr_t, but right now this was simple to do.
    pub fn clone_from_ptr(raw: *const ipv6_addr_t) -> Self {
        IPv6Addr {
            inner: unsafe { *raw },
        }
    }
}

#[cfg(feature = "with_embedded_nal")]
impl From<embedded_nal::Ipv6Addr> for IPv6Addr {
    fn from(input: embedded_nal::Ipv6Addr) -> Self {
        IPv6Addr {
            inner: ipv6_addr_t {
                u8_: input.octets(),
            },
        }
    }
}

#[cfg(feature = "with_embedded_nal")]
impl From<IPv6Addr> for embedded_nal::Ipv6Addr {
    fn from(addr: IPv6Addr) -> Self {
        Self::from(unsafe { addr.inner.u8_ })
    }
}

/// Given an address like fe80::1%42, split it up into a IPv6Addr and a numeric interface
/// identifier, if any is given. It is an error for the address not to be parsable, or for the
/// interface identifier not to be numeric.
///
/// Don't consider the error type final, that's just what works easily Right Now.
// This is not implemented in terms of the RIOT ipv6_addr functions (ipv6_addr_split_iface) as they
// heavily rely on null-terminated strings and mutating memory.
pub fn split_ipv6_address(input: &str) -> Result<(IPv6Addr, Option<kernel_pid_t>), &'static str> {
    let mut s = input.splitn(2, "%");
    let addr = s
        .next()
        .ok_or("No address")?
        .parse()
        .map_err(|_| "Unparsable address")?;
    let interface = match s.next() {
        None => None,
        Some(x) => Some(x.parse().map_err(|_| "Non-numeric interface identifier")?),
    };

    Ok((addr, interface))
}
