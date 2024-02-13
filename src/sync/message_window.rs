use core::cell::RefCell;
use core::mem;
use core::pin::Pin;
use core::ptr::{self, NonNull};
use core::task::{Context, Poll, Waker};

use std::sync::OnceLock;

use futures::Stream;
use widestring::{u16cstr, U16CStr};

use winapi::ctypes::c_void;
use winapi::shared::minwindef::{HINSTANCE__, LPARAM, LRESULT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::WM_CREATE;
use winapi::um::winuser::{CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassExW, SetWindowLongPtrW, CREATESTRUCTW, GWLP_USERDATA, HWND_MESSAGE, WNDCLASSEXW};

pub trait FromMessage: Sized {
    unsafe fn from_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<Self>;
}

pub struct MessageWindow<Msg> {
    hwnd: HWND,
    inner: InnerPtr<Msg>,
}

type InnerPtr<Msg> = NonNull<RefCell<Inner<Msg>>>;

struct Inner<Msg> {
    message: Option<Msg>,
    waker: Option<Waker>,
}

impl<Msg: FromMessage> MessageWindow<Msg> {
    pub fn new() -> Self {
        let class = get_class::<Msg>();

        let inner = Box::new(RefCell::new(Inner {
            message: None,
            waker: None,
        }));

        let inner = NonNull::new(Box::into_raw(inner)).unwrap();

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                u16cstr!("").as_ptr(),
                0, 0, 0, 0, 0,
                HWND_MESSAGE,
                ptr::null_mut(),
                get_instance(),
                inner.as_ptr().cast::<c_void>(),
            )
        };

        if hwnd == ptr::null_mut() {
            // take last error first
            let error = unsafe { GetLastError() };

            // then free the box we just allocated
            unsafe { drop(Box::from_raw(inner.as_ptr())); }

            panic!("CreateWindowExW error: {error}");
        }

        MessageWindow {
            hwnd,
            inner,
        }
    }
}

impl<Msg> MessageWindow<Msg> {
    pub fn handle(&self) -> HWND {
        self.hwnd
    }
}

impl<Msg: FromMessage> Stream for MessageWindow<Msg> {
    type Item = Msg;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let inner = unsafe { self.inner.as_ref() };
        let mut inner = inner.borrow_mut();

        if let Some(msg) = inner.message.take() {
            return Poll::Ready(Some(msg));
        }

        inner.waker.replace(cx.waker().clone());
        Poll::Pending
    }
}

impl<Msg> Drop for MessageWindow<Msg> {
    fn drop(&mut self) {
        unsafe {
            // clear dispatch ptr before destroying window, to ensure that
            // from here on out we don't risk any dangling references
            swap_inner_ptr::<Msg>(self.hwnd, None);

            // destroy the window
            DestroyWindow(self.hwnd);

            // finally drop the inner struct in place
            drop(Box::from_raw(self.inner.as_ptr()));
        }
    }
}

unsafe extern "system" fn wnd_proc<Msg: FromMessage>(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Some(inner) = get_inner_ptr(hwnd) else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };

    if msg == WM_CREATE {
        let params = &*(lparam as *const CREATESTRUCTW);
        let ptr = InnerPtr::<Msg>::new(params.lpCreateParams.cast());
        swap_inner_ptr(hwnd, ptr);
    }

    let Some(message) = Msg::from_message(hwnd, msg, wparam, lparam) else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };

    let mut inner = inner.as_ref().borrow_mut();

    if inner.message.replace(message).is_some() {
        log::debug!("dropped previous message, not received on time");
    }

    if let Some(waker) = inner.waker.take() {
        waker.wake();
    }

    0
}

unsafe fn get_inner_ptr<Msg>(hwnd: HWND) -> Option<InnerPtr<Msg>> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    InnerPtr::new(ptr as *mut _)
}

#[cfg(target_pointer_width = "64")]
type WindowLongPtr = winapi::shared::basetsd::LONG_PTR;

#[cfg(target_pointer_width = "32")]
type WindowLongPtr = winapi::um::winnt::LONG;

unsafe fn swap_inner_ptr<Msg>(hwnd: HWND, ptr: Option<InnerPtr<Msg>>) -> Option<InnerPtr<Msg>> {
    let ptr = ptr.map(|ptr| ptr.as_ptr()).unwrap_or(ptr::null_mut());
    let prev = SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as WindowLongPtr);
    InnerPtr::new(prev as *mut _)
}

fn get_class<Msg: FromMessage>() -> &'static U16CStr {
    static CLASS: OnceLock<&'static U16CStr> = OnceLock::new();

    let atom = CLASS.get_or_init(|| {
        let class_name = u16cstr!("windows_executor::sync::message_window");

        let class = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc::<Msg>),
            hInstance: get_instance(),
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };

        if unsafe { RegisterClassExW(&class) } == 0 {
            let error = unsafe { GetLastError() };
            panic!("RegisterClassExW error: {error}");
        }

        class_name
    });

    *atom
}

fn get_instance() -> *mut HINSTANCE__ {
    unsafe { GetModuleHandleW(ptr::null()) }
}
