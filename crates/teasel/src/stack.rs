//! How far a read may take the stack.

/// The stack a read may use below where it starts. A Rust thread has 2 MB unless it asks for more,
/// a Node worker's 4 MB; under WebAssembly, V8's stack may run out first, which the package
/// answers as `nesting_depth` too.
pub(crate) const BUDGET: usize = 1 << 20;

/// Where the stack stands.
#[inline(always)]
pub(crate) fn position() -> usize {
	let at: usize;
	#[cfg(target_arch = "x86_64")]
	unsafe {
		std::arch::asm!("mov {}, rsp", out(reg) at, options(pure, nomem, nostack, preserves_flags));
	}
	#[cfg(target_arch = "aarch64")]
	unsafe {
		std::arch::asm!("mov {}, sp", out(reg) at, options(pure, nomem, nostack, preserves_flags));
	}
	#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
	{
		let here = 0u8;
		at = std::hint::black_box(&here) as *const u8 as usize;
	}
	at
}

/// The lowest position a read that starts here may reach.
pub(crate) fn floor() -> usize {
	position().saturating_sub(BUDGET)
}
