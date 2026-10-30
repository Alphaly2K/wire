//! Bounded in-engine byte queues. Guest memory and host stdio stay in adapters.
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
pub(crate) const CAPACITY: usize = 8 * 1024 * 1024;

pub(crate) fn reserve(queue: &mut VecDeque<u8>, length: usize) -> Result<(), u32> {
    if length > queue.capacity() {
        queue
            .try_reserve_exact(length.next_power_of_two() - queue.len())
            .map_err(|_| 8u32)?;
    }
    Ok(())
}

pub(crate) fn peek(queue: &VecDeque<u8>, count: usize) -> Result<(Vec<u8>, usize), u32> {
    let mut bytes = Vec::new();
    let count = count.min(queue.len());
    bytes.try_reserve_exact(count).map_err(|_| 8u32)?;
    bytes.extend(queue.iter().take(count));
    Ok((bytes, queue.len()))
}

#[derive(Clone, Default)]
pub(crate) struct Streams {
    outputs: Rc<RefCell<[VecDeque<u8>; 2]>>,
}
impl Streams {
    pub(crate) fn available(&self, index: usize) -> usize {
        CAPACITY - self.outputs.borrow()[index - 1].len()
    }
    pub(crate) fn append_with<E>(
        &mut self,
        index: usize,
        length: usize,
        mut read: impl FnMut(usize, &mut [u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        if length == 0 {
            return Ok(true);
        }
        let mut outputs = self.outputs.borrow_mut();
        let output = &mut outputs[index - 1];
        if length > CAPACITY - output.len() || reserve(output, output.len() + length).is_err() {
            return Ok(false);
        }
        let start = output.len();
        output.resize(start + length, 0);
        let result = (|| {
            let (first, second) = output.as_mut_slices();
            if start < first.len() {
                let count = (first.len() - start).min(length);
                read(0, &mut first[start..start + count])?;
                read(count, &mut second[..length - count])
            } else {
                read(
                    0,
                    &mut second[start - first.len()..start - first.len() + length],
                )
            }
        })();
        if let Err(error) = result {
            output.truncate(start);
            return Err(error);
        }
        Ok(true)
    }
    pub(crate) fn read(&mut self, index: u32, output: &mut [u8]) -> Result<usize, String> {
        let mut outputs = self.outputs.borrow_mut();
        let input = outputs
            .get_mut(index.wrapping_sub(1) as usize)
            .ok_or("invalid standard output endpoint")?;
        let count = output.len().min(input.len());
        let (first, second) = input.as_slices();
        let start = count.min(first.len());
        output[..start].copy_from_slice(&first[..start]);
        output[start..count].copy_from_slice(&second[..count - start]);
        input.drain(..count);
        if input.is_empty() {
            *input = VecDeque::new();
        }
        Ok(count)
    }
}

#[derive(Default)]
struct InputData {
    data: VecDeque<u8>,
    ended: bool,
}
#[derive(Clone, Default)]
pub(crate) struct Input(Rc<RefCell<InputData>>);
impl Input {
    pub(crate) fn peek(&self, count: usize) -> Result<(Vec<u8>, usize), u32> {
        let input = self.0.borrow();
        if input.ended && input.data.is_empty() {
            return Err(109);
        }
        peek(&input.data, count)
    }
    pub(crate) fn write(&mut self, bytes: &[u8], end: bool) -> Result<(), u32> {
        let mut input = self.0.borrow_mut();
        if input.ended && !bytes.is_empty() {
            return Err(87);
        }
        if bytes.len() > CAPACITY - input.data.len() {
            return Err(8);
        }
        let length = input.data.len() + bytes.len();
        reserve(&mut input.data, length)?;
        input.data.extend(bytes);
        input.ended |= end;
        Ok(())
    }
    pub(crate) fn read(&mut self, count: usize) -> Result<Option<Vec<u8>>, u32> {
        let mut input = self.0.borrow_mut();
        if count != 0 && input.data.is_empty() && !input.ended {
            return Ok(None);
        }
        let count = count.min(input.data.len());
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(count).map_err(|_| 8u32)?;
        bytes.extend(input.data.drain(..count));
        if input.data.is_empty() {
            input.data = VecDeque::new();
        }
        Ok(Some(bytes))
    }
    pub(crate) fn close(&mut self) {
        self.0 = Rc::new(RefCell::new(InputData {
            data: VecDeque::new(),
            ended: true,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_endpoint_views_share_queues_without_sharing_local_close() {
        let mut parent = Input::default();
        let mut child = parent.clone();
        parent.write(&[1, 2, 3], false).unwrap();
        assert_eq!(child.read(1), Ok(Some(vec![1])));
        child.close();
        assert_eq!(child.read(1), Ok(Some(vec![])));
        assert_eq!(parent.read(2), Ok(Some(vec![2, 3])));
        assert_eq!(parent.read(1), Ok(None));
        let mut parent = Streams::default();
        let mut child = parent.clone();
        assert!(child
            .append_with::<()>(1, 3, |offset, output| {
                output.copy_from_slice(&[4, 5, 6][offset..offset + output.len()]);
                Ok(())
            })
            .unwrap());
        let mut output = [0; 8];
        assert_eq!(parent.read(1, &mut output).unwrap(), 3);
        assert_eq!(&output[..3], &[4, 5, 6]);
        assert_eq!(child.read(1, &mut output).unwrap(), 0);
    }
}
