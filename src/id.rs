#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(usize);

#[derive(Default)]
pub struct IdGenerator {
    state: usize,
}

impl IdGenerator {
    pub fn next(&mut self) -> Id {
        let res = Id(self.state);
        self.state += 1;
        res
    }
}
