use std::cell::UnsafeCell;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::num::NonZero;
use std::sync::atomic;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;

use bytemuck::AnyBitPattern;
use bytemuck::NoUninit;
use interprocess::unnamed_pipe::Recver as ReceiverU8;
use interprocess::unnamed_pipe::Sender as SenderU8;

use crossbeam::channel::Receiver as MpscReceiver;
use crossbeam::channel::Sender as MpscSender;

pub fn bounded<T: Send + NoUninit + AnyBitPattern>(
    capacity_hint: usize,
) -> std::io::Result<(Sender<T>, Receiver<T>)> {
    let (sender, receiver) = interprocess::os::windows::unnamed_pipe::CreationOptions::new()
        .buffer_size_hint(NonZero::new(capacity_hint * std::mem::size_of::<T>()))
        .inheritable(true)
        .build()?;
    let sender = Sender::new(sender);
    let receiver = Receiver::new(receiver);
    Ok((sender, receiver))
}

pub struct Sender<T: Send + NoUninit + AnyBitPattern> {
    sender: UnsafeCell<BufWriter<SenderU8>>,
    _phantom_data: std::marker::PhantomData<T>,
}

impl<T: Send + NoUninit + AnyBitPattern> Sender<T> {
    fn new(sender: SenderU8) -> Self {
        Self {
            sender: UnsafeCell::new(BufWriter::new(sender)),
            _phantom_data: Default::default(),
        }
    }

    pub fn send(&self, msg: T) -> Result<(), T> {
        let sender = self.sender.get();
        unsafe { (*sender).write_all(bytemuck::bytes_of(&msg)) }.map_err(|_| msg)?;
        unsafe { (*sender).flush() }.map_err(|_| msg)?;
        Ok(())
    }
}

impl<T: Send + NoUninit + AnyBitPattern> From<std::os::windows::io::OwnedHandle> for Sender<T> {
    fn from(value: std::os::windows::io::OwnedHandle) -> Self {
        Self::new(value.into())
    }
}

impl<T: Send + NoUninit + AnyBitPattern> Into<std::os::windows::io::OwnedHandle> for Sender<T> {
    fn into(self) -> std::os::windows::io::OwnedHandle {
        self.sender.into_inner().into_inner().unwrap().into()
    }
}

pub struct Receiver<T: Send + NoUninit + AnyBitPattern> {
    receiver: UnsafeCell<BufReader<ReceiverU8>>,
    buf: UnsafeCell<Box<[u8]>>,
    _phantom_data: std::marker::PhantomData<T>,
}

impl<T: Send + NoUninit + AnyBitPattern> Receiver<T> {
    fn new(receiver: ReceiverU8) -> Self {
        Self {
            receiver: UnsafeCell::new(BufReader::new(receiver)),
            buf: UnsafeCell::new((0..std::mem::size_of::<T>()).map(|_| 0).collect()),
            _phantom_data: Default::default(),
        }
    }

    pub fn recv(&self) -> std::io::Result<T> {
        let receiver = self.receiver.get();
        let buf = self.buf.get();
        unsafe { (*receiver).read_exact(&mut *buf) }?;
        let msg = bytemuck::from_bytes(unsafe { &(*buf) });
        let msg = unsafe { std::ptr::read(msg as _) };
        Ok(msg)
    }
}

impl<T: Send + NoUninit + AnyBitPattern> From<std::os::windows::io::OwnedHandle> for Receiver<T> {
    fn from(value: std::os::windows::io::OwnedHandle) -> Self {
        Self::new(value.into())
    }
}

impl<T: Send + NoUninit + AnyBitPattern> Into<std::os::windows::io::OwnedHandle> for Receiver<T> {
    fn into(self) -> std::os::windows::io::OwnedHandle {
        self.receiver.into_inner().into_inner().into()
    }
}

pub struct NonBlockSender<T: Send + NoUninit + AnyBitPattern> {
    mpsc_sender: MpscSender<T>,
    is_running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + NoUninit + AnyBitPattern> NonBlockSender<T> {
    pub fn bounded(sender: Sender<T>, cap: usize) -> Self {
        let (mpsc_sender, mpsc_receiver) = crossbeam::channel::bounded(cap);
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_1 = is_running.clone();
        let thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let is_running = is_running_1;
            while is_running.load(atomic::Ordering::Relaxed) {
                mpsc_receiver.iter().next().map(|msg| {
                    buf.push(msg);
                });
                buf.extend(mpsc_receiver.try_iter());
                buf.drain(..).for_each(|msg| {
                    sender.send(msg).map_err(|_| ()).unwrap();
                });
            }
        });
        Self {
            mpsc_sender,
            is_running,
            thread: Some(thread),
        }
    }

    pub fn send(&self, msg: T) -> Result<(), crossbeam::channel::SendError<T>> {
        self.mpsc_sender.send(msg)
    }
}

impl<T: Send + NoUninit + AnyBitPattern> Drop for NonBlockSender<T> {
    fn drop(&mut self) {
        self.is_running.store(false, atomic::Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

pub struct NonBlockReceiver<T: Send + NoUninit + AnyBitPattern> {
    mpsc_receiver: MpscReceiver<T>,
    is_running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + NoUninit + AnyBitPattern> NonBlockReceiver<T> {
    pub fn bounded(receiver: Receiver<T>, cap: usize) -> Self {
        let (mpsc_sender, mpsc_receiver) = crossbeam::channel::bounded(cap);
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_1 = is_running.clone();
        let thread = std::thread::spawn(move || {
            let is_running = is_running_1;
            while is_running.load(atomic::Ordering::Relaxed) {
                let Ok(msg) = receiver.recv() else {
                    break;
                };
                mpsc_sender.send(msg).unwrap();
            }
        });
        Self {
            mpsc_receiver,
            is_running,
            thread: Some(thread),
        }
    }

    pub fn try_iter(&self) -> crossbeam::channel::TryIter<'_, T> {
        self.mpsc_receiver.try_iter()
    }
}

impl<T: Send + NoUninit + AnyBitPattern> Drop for NonBlockReceiver<T> {
    fn drop(&mut self) {
        self.is_running.store(false, atomic::Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
