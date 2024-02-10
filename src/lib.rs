#![cfg_attr(not(test), no_std)]

mod waker;

use core::future::Future;
use core::mem::MaybeUninit;
use core::ptr;
use core::task::{Context, Poll};

use winapi::um::errhandlingapi::GetLastError;
use winapi::um::winuser::{DispatchMessageW, GetMessageW, TranslateMessage, MSG};

pub type LoopResult<T> = Result<T, ShouldExit>;

#[derive(Debug, Clone, Copy)]
pub struct ShouldExit;

pub fn block_on<T>(fut: impl Future<Output = T>) -> LoopResult<T> {
    futures::pin_mut!(fut);

    let waker = waker::for_current_thread();
    let mut context = Context::from_waker(&waker);

    loop {
        if let Poll::Ready(value) = fut.as_mut().poll(&mut context) {
            return Ok(value);
        }

        unsafe {
            let mut msg = MaybeUninit::<MSG>::uninit();

            let ret = GetMessageW(
                msg.as_mut_ptr(),
                ptr::null_mut(),
                0,
                0,
            );

            if ret == -1 {
                let error = GetLastError();
                panic!("GetMessageW failed: {error}");
            } else if ret == 0 {
                return Err(ShouldExit);
            }

            let msg = msg.assume_init();
            log::debug!("dispatching message: hwnd={:?}, msg={}", msg.hwnd, msg.message);
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::future;
    use core::task::Poll;
    use crate::block_on;

    #[test]
    fn it_wakes() {
        let mut polls = 0;

        let fut = future::poll_fn(|cx| {
            polls += 1;
            match polls {
                1 => {
                    cx.waker().clone().wake();
                    Poll::Pending
                }
                2 => {
                    Poll::Ready(())
                }
                _ => {
                    panic!("polled too many times!")
                }
            }
        });

        assert_eq!((), block_on(fut).unwrap());
        assert_eq!(2, polls);
    }
}
