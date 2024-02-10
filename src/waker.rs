use core::ptr;
use core::task::{RawWakerVTable, RawWaker, Waker};

use winapi::shared::minwindef::FALSE;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::{CloseHandle, DuplicateHandle};
use winapi::um::processthreadsapi::{GetCurrentProcess, GetCurrentThread, GetThreadId};
use winapi::um::winnt::{DUPLICATE_SAME_ACCESS, HANDLE};
use winapi::um::winuser::{PostThreadMessageW, WM_NULL};

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone,
    wake,
    wake_by_ref,
    drop,
);

pub fn for_current_thread() -> Waker {
    unsafe { new(GetCurrentThread()) }
}

pub unsafe fn new(thread: HANDLE) -> Waker {
    unsafe { Waker::from_raw(new_raw(thread)) }
}

pub unsafe fn new_raw(thread: HANDLE) -> RawWaker {
    let mut handle = ptr::null_mut();

    let rc = DuplicateHandle(
        GetCurrentProcess(),
        thread,
        GetCurrentProcess(),
        &mut handle,
        0,
        FALSE,
        DUPLICATE_SAME_ACCESS,
    );

    if rc != 1 {
        let error = GetLastError();
        panic!("DuplicateHandle failed: {error}");
    }

    RawWaker::new(handle as *const (), &VTABLE)
}

unsafe fn clone(ptr: *const ()) -> RawWaker {
    let handle = ptr as HANDLE;
    new_raw(handle)
}

unsafe fn wake(ptr: *const ()) {
    wake_by_ref(ptr);
    drop(ptr);
}

unsafe fn wake_by_ref(ptr: *const ()) {
    let handle = ptr as HANDLE;

    let thread_id = GetThreadId(handle);
    if thread_id == 0 {
        let error = GetLastError();
        log::debug!("GetThreadId error in wake_by_ref: {error}");
        return;
    }

    let rc = PostThreadMessageW(thread_id, WM_NULL, 0, 0);
    if rc != 1 {
        let error = GetLastError();
        log::debug!("PostThreadMessageW error in wake_by_ref: {error}");
    }
}

unsafe fn drop(ptr: *const ()) {
    let handle = ptr as HANDLE;
    CloseHandle(handle);
}
