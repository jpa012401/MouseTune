use crossbeam_channel::Receiver;

use super::{ActionSink, Job};

pub const QUEUE_CAPACITY: usize = 64;

pub struct ChannelSink(crossbeam_channel::Sender<Job>);

impl ActionSink for ChannelSink {
    /// Never blocks or allocates: the channel's buffer is preallocated.
    fn submit(&self, job: Job) -> bool {
        self.0.try_send(job).is_ok()
    }
}

pub fn queue() -> (ChannelSink, Receiver<Job>) {
    let (tx, rx) = crossbeam_channel::bounded(QUEUE_CAPACITY);
    (ChannelSink(tx), rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_queue_refuses_without_blocking() {
        let (sink, _rx) = queue();
        for _ in 0..QUEUE_CAPACITY {
            assert!(sink.submit(Job::Click { button: 2 }));
        }
        let started = std::time::Instant::now();
        assert!(!sink.submit(Job::Click { button: 2 }));
        assert!(started.elapsed() < std::time::Duration::from_millis(10));
    }

    #[test]
    fn a_closed_queue_refuses() {
        let (sink, rx) = queue();
        drop(rx);
        assert!(!sink.submit(Job::Click { button: 2 }));
    }
}
