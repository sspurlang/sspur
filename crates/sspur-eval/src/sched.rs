// The sequential scheduler in the browser leaves the thread hand-off unused.
#![cfg_attr(target_family = "wasm", allow(dead_code))]
use crate::value::{AtomCell, ChanCell, Value};
use crate::{trap, Ctrl, Interp, R};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::{Condvar, Mutex};

pub const DEADLOCK: &str = "deadlock: every task is blocked on recv";
pub const SEND_CLOSED: &str = "send on a closed channel";
#[cfg(target_family = "wasm")]
pub const PAR_SEQUENTIAL: &str = "recv would wait for a task that hasn't started: the playground runs par tasks one after another, so this program needs the native build";
const TASK_STACK: usize = 1 << 29;

#[derive(Default)]
pub(crate) struct Sched {
    m: Mutex<State>,
    cv: Condvar,
}

#[derive(Default)]
struct State {
    current: usize,
    next_id: usize,
    ready: VecDeque<usize>,
    blocked: Vec<usize>,
    dead: HashSet<usize>,
    remaining: HashMap<usize, usize>,
    next_obj: u64,
    /// Tasks not yet started in the sequential scheduler.
    #[cfg(target_family = "wasm")]
    later: usize,
}

impl State {
    fn pick(&mut self) -> usize {
        if self.ready.is_empty() {
            for t in std::mem::take(&mut self.blocked) {
                self.dead.insert(t);
                self.ready.push_back(t);
            }
        }
        self.ready.pop_front().unwrap_or(self.current)
    }
}

struct Unsafe<T>(T);
unsafe impl<T> Send for Unsafe<T> {}
impl<T> Unsafe<T> {
    fn get(self) -> T {
        self.0
    }
}

impl Interp {
    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.sched.m.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn switch(&self, me: Option<usize>) {
        let handlers = self.handlers.take();
        let depth = self.depth.get();
        let mut s = self.st();
        let next = s.pick();
        s.current = next;
        self.sched.cv.notify_all();
        if let Some(me) = me {
            while s.current != me {
                s = self.sched.cv.wait(s).unwrap_or_else(|e| e.into_inner());
            }
            drop(s);
            self.handlers.replace(handlers);
            self.depth.set(depth);
        }
    }

    fn wait_turn(&self, me: usize) {
        let mut s = self.st();
        while s.current != me {
            s = self.sched.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(crate) fn new_obj_id(&self) -> u64 {
        let mut s = self.st();
        s.next_obj += 1;
        s.next_obj
    }

    pub(crate) fn new_atomic(&self, v: i64) -> Value {
        Value::Atomic(Rc::new(AtomCell { id: self.new_obj_id(), v: Cell::new(v) }))
    }

    pub(crate) fn new_chan(&self) -> Value {
        Value::Chan(Rc::new(ChanCell { id: self.new_obj_id(), q: RefCell::new(VecDeque::new()), closed: Cell::new(false), waiters: RefCell::new(VecDeque::new()) }))
    }

    pub(crate) fn run_tasks(&self, n: usize, body: &impl Fn(usize) -> R) -> R<Vec<Value>> {
        if n == 0 {
            return Ok(vec![]);
        }
        let (me, ids) = {
            let mut s = self.st();
            let me = s.current;
            let ids: Vec<usize> = (0..n)
                .map(|_| {
                    s.next_id += 1;
                    s.next_id
                })
                .collect();
            s.remaining.insert(me, n);
            s.ready.extend(ids.iter().copied());
            (me, ids)
        };
        let results = self.run_bodies(me, &ids, body);
        let mut out = Vec::with_capacity(n);
        let mut first_err: Option<Ctrl> = None;
        let mut deadlock: Option<Ctrl> = None;
        for slot in results {
            match slot.into_inner().unwrap_or_else(|| trap("task did not finish")) {
                Ok(v) => out.push(v),
                Err(Ctrl::Trap(m)) if m == DEADLOCK => {
                    if deadlock.is_none() {
                        deadlock = Some(Ctrl::Trap(m));
                    }
                }
                Err(c) => {
                    if first_err.is_none() {
                        first_err = Some(c);
                    }
                }
            }
        }
        match first_err.or(deadlock) {
            Some(c) => Err(c),
            None => Ok(out),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn run_bodies(&self, me: usize, ids: &[usize], body: &dyn Fn(usize) -> R) -> Vec<RefCell<Option<R>>> {
        let n = ids.len();
        let depth = self.depth.get();
        let results: Vec<RefCell<Option<R>>> = (0..n).map(|_| RefCell::new(None)).collect();
        std::thread::scope(|sc| {
            for (k, &id) in ids.iter().enumerate() {
                let job = Unsafe((self as *const Interp, body as *const dyn Fn(usize) -> R, &results[k] as *const RefCell<Option<R>>));
                let r = std::thread::Builder::new().stack_size(TASK_STACK).spawn_scoped(sc, move || {
                    let (it, body, slot) = job.get();
                    let (it, body, slot) = unsafe { (&*it, &*body, &*slot) };
                    crate::stack_from_here(TASK_STACK - (64 << 10));
                    it.wait_turn(id);
                    it.depth.set(depth);
                    let r = body(k);
                    *slot.borrow_mut() = Some(r);
                    {
                        let mut s = it.st();
                        let left = s.remaining.get_mut(&me).map(|c| {
                            *c -= 1;
                            *c
                        });
                        if left == Some(0) {
                            s.remaining.remove(&me);
                            s.ready.push_back(me);
                        }
                    }
                    it.handlers.take();
                    it.switch(None);
                });
                if r.is_err() {
                    let mut s = self.st();
                    s.ready.retain(|t| !ids[k..].contains(t));
                    let left = s.remaining.get_mut(&me).map(|c| {
                        *c -= n - k;
                        *c
                    });
                    if left == Some(0) {
                        s.remaining.remove(&me);
                        s.ready.push_back(me);
                    }
                    drop(s);
                    for slot in &results[k..] {
                        *slot.borrow_mut() = Some(trap("out of memory"));
                    }
                    break;
                }
            }
            self.switch(Some(me));
        });
        results
    }

    /// Without threads every task runs to completion in spawn order, which is the order the
    /// threaded scheduler starts them in. A `recv` that would wait for a later task can't.
    /// Generic, not `dyn`, so the asyncify pass sees that a body can hop (see hop.rs).
    #[cfg(target_family = "wasm")]
    fn run_bodies(&self, me: usize, ids: &[usize], body: &impl Fn(usize) -> R) -> Vec<RefCell<Option<R>>> {
        let depth = self.depth.get();
        let handlers = self.handlers.take();
        let mut results = Vec::with_capacity(ids.len());
        for (k, &id) in ids.iter().enumerate() {
            {
                let mut s = self.st();
                s.ready.retain(|t| *t != id);
                s.current = id;
                s.later += ids.len() - k - 1;
            }
            self.depth.set(depth);
            let r = body(k);
            self.handlers.take();
            self.st().later -= ids.len() - k - 1;
            results.push(RefCell::new(Some(r)));
        }
        let mut s = self.st();
        s.remaining.remove(&me);
        s.ready.retain(|t| *t != me);
        s.current = me;
        drop(s);
        self.handlers.replace(handlers);
        self.depth.set(depth);
        results
    }

    pub(crate) fn chan_send(&self, c: &ChanCell, v: Value) -> R {
        if c.closed.get() {
            return trap(SEND_CLOSED);
        }
        c.q.borrow_mut().push_back(v);
        let w = c.waiters.borrow_mut().pop_front();
        if let Some(w) = w {
            let mut s = self.st();
            s.blocked.retain(|t| *t != w);
            s.ready.push_back(w);
        }
        Ok(Value::Unit)
    }

    pub(crate) fn chan_close(&self, c: &ChanCell) -> R {
        c.closed.set(true);
        let ws: Vec<usize> = c.waiters.borrow_mut().drain(..).collect();
        if !ws.is_empty() {
            let mut s = self.st();
            s.blocked.retain(|t| !ws.contains(t));
            s.ready.extend(ws);
        }
        Ok(Value::Unit)
    }

    #[cfg(target_family = "wasm")]
    pub(crate) fn chan_recv(&self, c: &ChanCell) -> R<Option<Value>> {
        if let Some(v) = c.q.borrow_mut().pop_front() {
            return Ok(Some(v));
        }
        if c.closed.get() {
            return Ok(None);
        }
        trap(if self.st().later > 0 { PAR_SEQUENTIAL } else { DEADLOCK })
    }

    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn chan_recv(&self, c: &ChanCell) -> R<Option<Value>> {
        loop {
            if let Some(v) = c.q.borrow_mut().pop_front() {
                return Ok(Some(v));
            }
            if c.closed.get() {
                return Ok(None);
            }
            let me = {
                let mut s = self.st();
                let cur = s.current;
                s.blocked.push(cur);
                s.current
            };
            c.waiters.borrow_mut().push_back(me);
            self.switch(Some(me));
            if self.st().dead.remove(&me) {
                c.waiters.borrow_mut().retain(|t| *t != me);
                return trap(DEADLOCK);
            }
        }
    }
}
