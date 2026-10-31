//! The Roblox-style Luau binding. It is *generic*: it exposes whatever the class
//! registry describes (properties, signals, methods) and knows no class by name.
//!
//! Every instance seen by Lua is a userdata holding only an [`Id`]; all state lives
//! in the data model. Userdata are cached per instance (weak table), so `a == b`
//! and table keys work like in Roblox.

use std::{
    cell::{Cell, RefCell},
    collections::{BinaryHeap, HashMap},
    rc::Rc,
};

use bevy::{
    input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput},
    math::Vec2,
};
use mlua::{AnyUserData, ChunkMode, Compiler, Function, Lua, MetaMethod, MultiValue, Result as LuaResult, Table, Thread, UserData, UserDataFields, UserDataMethods, Value, Variadic};

use super::{keys, lua_types::*};
use crate::{
    assets::{FileStamp, GameSource},
    core::{data_type, enum_names, FastMap, HostServices, Id, MethodCall, MethodDesc, PropType, ResolvedClass, ScriptValue, Value as DV},
    dm::DataModel,
    physics::PhysicsBridge,
};

#[derive(Default)]
pub struct InputState {
    pub keys: ButtonInput<KeyCode>,
    pub mouse: ButtonInput<MouseButton>,
    pub delta: Vec2,
    pub pos: Vec2,
}

/// A thread asleep in `task.wait`.
pub struct Waiting {
    pub thread: Thread,
    pub wake: f64,
    pub started: f64,
    /// Counts the sleepers in the order they went to sleep (what breaks ties between equal wake times). Deferred threads
    /// (`task.defer`) count from far below, so they come before every thread that waited.
    seq: i64,
}

impl PartialEq for Waiting {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Waiting {}

impl PartialOrd for Waiting {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Reversed, so the std max-heap pops the earliest wake-up first.
impl Ord for Waiting {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.wake.total_cmp(&self.wake).then(other.seq.cmp(&self.seq))
    }
}

/// The threads waiting in `task.wait`, earliest wake-up first. A frame looks only at the sleepers
/// that are due (a few), not at every thread that is waiting (possibly tens of thousands asleep for seconds).
#[derive(Default)]
pub struct Sleepers {
    heap: BinaryHeap<Waiting>,
    added: i64,
    deferred: i64,
}

impl Sleepers {
    pub fn add(&mut self, thread: Thread, now: f64, secs: f64) {
        self.added += 1;
        self.heap.push(Waiting { thread, wake: now + secs.max(0.0), started: now, seq: self.added });
    }

    /// A thread for `task.defer`: resumed at the next resume point, ahead of the threads that are waiting.
    pub fn defer(&mut self, thread: Thread, now: f64) {
        self.deferred += 1;
        self.heap.push(Waiting { thread, wake: now, started: now, seq: i64::MIN / 2 + self.deferred });
    }

    /// Removes the sleepers whose time has come. They come out in the order they went to sleep
    /// (not by wake time), which is the order scripts have always been resumed in.
    pub fn take_due(&mut self, now: f64) -> Vec<Waiting> {
        let mut due = Vec::new();
        while self.heap.peek().is_some_and(|w| w.wake <= now) {
            due.extend(self.heap.pop());
        }
        due.sort_unstable_by_key(|w| w.seq);
        due
    }

    pub fn clear(&mut self) {
        self.heap.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thread that returns `name` when resumed, so a test can tell which sleeper it got back.
    fn sleeper(lua: &Lua, name: &str) -> Thread {
        lua.create_thread(lua.load(format!("return '{name}'")).into_function().unwrap()).unwrap()
    }

    /// The sleepers due at `now`, in the order they would be resumed.
    fn due(sleepers: &mut Sleepers, now: f64) -> Vec<String> {
        sleepers.take_due(now).into_iter().map(|w| w.thread.resume::<String>(()).unwrap()).collect()
    }

    #[test]
    fn only_sleepers_that_are_due_come_out_and_in_the_order_they_went_to_sleep() {
        let lua = Lua::new();
        let mut s = Sleepers::default();
        s.add(sleeper(&lua, "long"), 0.0, 1.0);
        s.add(sleeper(&lua, "short"), 0.0, 0.5);
        s.add(sleeper(&lua, "much later"), 0.0, 5.0);
        s.add(sleeper(&lua, "now"), 0.0, 0.0);
        s.add(sleeper(&lua, "negative"), 0.0, -3.0);
        assert_eq!(due(&mut s, 0.0), ["now", "negative"]);
        assert!(due(&mut s, 0.25).is_empty());
        assert_eq!(due(&mut s, 2.0), ["long", "short"], "in the order they went to sleep, not by wake time");
        assert!(due(&mut s, 2.0).is_empty(), "nobody is resumed twice");
        assert_eq!(due(&mut s, 10.0), ["much later"]);
    }

    #[test]
    fn clearing_forgets_everybody() {
        let lua = Lua::new();
        let mut s = Sleepers::default();
        s.add(sleeper(&lua, "a"), 0.0, 0.0);
        s.clear();
        assert!(due(&mut s, 1.0).is_empty());
    }
}

#[derive(Default)]
pub struct Requests {
    pub quit: bool,
    pub place: Option<String>,
}

pub struct Ctx {
    pub dm: Rc<RefCell<DataModel>>,
    pub physics: RefCell<PhysicsBridge>,
    pub source: GameSource,
    pub cache: Table,
    /// Methods available on every instance (added by Rust and by class snippets).
    pub methods: Table,
    /// Luau-implemented methods per class name.
    pub class_methods: Table,
    pub enums: Table,
    pub signal_new: Function,
    pub signal_fire: Function,
    pub new_env: Function,
    /// The signals scripts have asked for, by instance uid and then by name. (Two levels so a lookup
    /// can use the borrowed name: a flat `(uid, String)` key would allocate on every single lookup,
    /// and dropping a destroyed instance's signals would have to scan all of them.)
    pub signals: RefCell<FastMap<u32, FastMap<String, Table>>>,
    pub method_cache: RefCell<FastMap<usize, Function>>,
    pub input: RefCell<InputState>,
    pub time: Cell<f64>,
    pub dt: Cell<f32>,
    pub requests: RefCell<Requests>,
    pub sched: RefCell<Sleepers>,
    /// How the value types that are only numbers (`NumberSequence`, `BrickColor` ...) become numbers and back
    /// (`datatypes.luau`): `encode(tag, value)`, `decode(tag, numbers)`, and which of those types a value is.
    pub data_encode: Function,
    pub data_decode: Function,
    pub data_tag_of: Function,
    /// Gives a table the metatable of one of Roblox's result types (`set_type("RaycastResult", fields)`).
    pub set_type: Function,
    /// The functions scripts assigned to callback members (`BindableFunction.OnInvoke` ...), by instance uid
    /// and member name. Strong, so a callback outlives the script's references to the instance; dropped with
    /// the instance (see [`Ctx::forget_signals`]).
    pub callbacks: Table,
    /// Scripts that were started, by instance uid, with their main thread (so it can be stopped).
    pub running: RefCell<HashMap<u32, Thread>>,
    /// Compiled scripts by file path. Many instances usually share one file (a cloned enemy, a
    /// coin ...), and compiling is the expensive part of starting a script, so a file is compiled
    /// once and each instance loads the bytecode under its own chunk name.
    pub chunks: RefCell<HashMap<String, CompiledChunk>>,
    pub compiler: Compiler,
}

/// The bytecode of a script file and the state of the file it was compiled from.
pub struct CompiledChunk {
    stamp: FileStamp,
    bytecode: Rc<[u8]>,
    /// The game time at which the file was last found unchanged. A thousand scripts started in one
    /// frame look at the file once (a `stat` is a system call, and on Windows a slow one).
    checked: Cell<f64>,
}

impl HostServices for Ctx {
    fn key_down(&self, key: &str) -> bool {
        keys::BY_NAME.get(key).is_some_and(|k| self.input.borrow().keys.pressed(*k))
    }
    fn mouse_button_down(&self, button: &str) -> bool {
        keys::mouse_by_name(button).is_some_and(|b| self.input.borrow().mouse.pressed(b))
    }
    fn mouse_delta(&self) -> Vec2 {
        self.input.borrow().delta
    }
    fn mouse_position(&self) -> Vec2 {
        self.input.borrow().pos
    }
    fn quit(&self) {
        self.requests.borrow_mut().quit = true;
    }
    fn load_scene(&self, name: String) {
        self.requests.borrow_mut().place = Some(name);
    }
}

#[derive(Clone)]
pub struct InstanceRef {
    pub ctx: Rc<Ctx>,
    pub id: Id,
}

fn lstr(s: &mlua::String) -> LuaResult<String> {
    let b = s.to_str()?;
    let st: &str = &b;
    Ok(st.to_owned())
}

fn this(ud: &AnyUserData) -> LuaResult<InstanceRef> {
    let r = ud.borrow::<InstanceRef>()?;
    Ok(InstanceRef { ctx: r.ctx.clone(), id: r.id })
}

/// How deeply tables may nest in a method argument. Also what stops a table that contains
/// itself: converting it would otherwise recurse until the Rust stack overflows (an abort).
const MAX_ARG_DEPTH: usize = 32;

/// The `__type` of a table's metatable (`"EnumItem"`, `"UDim2"`, ...), if it has one.
fn table_type(t: &Table) -> Option<String> {
    t.metatable()?.raw_get::<Option<String>>("__type").ok().flatten()
}

impl Ctx {
    /// Lua value -> language-neutral value (for method arguments).
    fn lua_to_script(&self, v: &Value) -> LuaResult<ScriptValue> {
        self.lua_to_script_at(v, 0)
    }

    fn lua_to_script_at(&self, v: &Value, depth: usize) -> LuaResult<ScriptValue> {
        if depth > MAX_ARG_DEPTH {
            return Err(rt("argument tables are nested too deeply (or contain themselves)"));
        }
        Ok(match v {
            Value::Nil => ScriptValue::Nil,
            Value::Boolean(b) => ScriptValue::Bool(*b),
            Value::Number(n) => ScriptValue::Num(*n),
            Value::Integer(i) => ScriptValue::Num(*i as f64),
            Value::String(s) => ScriptValue::Str(lstr(s)?),
            Value::Vector(x) => ScriptValue::Vec3(unvec(x)),
            Value::UserData(ud) => {
                if let Ok(r) = ud.borrow::<InstanceRef>() {
                    ScriptValue::Instance(r.id)
                } else if let Some(cf) = as_cf(v) {
                    ScriptValue::CFrame(cf.p, cf.q)
                } else if let Some(c) = as_color(v) {
                    ScriptValue::Color(c.0)
                } else if let Some(v2) = as_v2(v) {
                    ScriptValue::Vec2(v2)
                } else {
                    return Err(rt("unsupported userdata argument"));
                }
            }
            Value::Table(t) => match table_type(t).as_deref() {
                Some("EnumItem") => ScriptValue::Enum(t.raw_get::<String>("Name")?),
                Some(tag) if data_type(tag).is_some() => self.data_to_script(tag, v)?,
                _ if t.raw_len() > 0 => {
                    let items: LuaResult<Vec<ScriptValue>> = t.sequence_values::<Value>().map(|x| self.lua_to_script_at(&x?, depth + 1)).collect();
                    ScriptValue::List(items?)
                }
                _ => {
                    let mut map = Vec::new();
                    for pair in t.pairs::<Value, Value>() {
                        let (k, val) = pair?;
                        if let Value::String(ks) = &k {
                            map.push((lstr(ks)?, self.lua_to_script_at(&val, depth + 1)?));
                        }
                    }
                    ScriptValue::Map(map)
                }
            },
            _ => return Err(rt("unsupported argument type")),
        })
    }

    /// A `NumberSequence`, `BrickColor` ... as a script value (its numbers).
    fn data_to_script(&self, tag: &str, v: &Value) -> LuaResult<ScriptValue> {
        let tag = data_type(tag).ok_or_else(|| rt(format!("{tag} cannot be stored")))?;
        Ok(ScriptValue::Data(tag, self.encode_data(tag, v)?))
    }

    fn encode_data(&self, tag: &str, v: &Value) -> LuaResult<Vec<f32>> {
        let numbers: Table = self.data_encode.call((tag, v.clone()))?;
        numbers.sequence_values::<f32>().collect()
    }

    fn decode_data(&self, lua: &Lua, tag: &str, numbers: &[f32]) -> LuaResult<Value> {
        let list = lua.create_sequence_from(numbers.iter().map(|n| f64::from(*n)))?;
        self.data_decode.call((tag, list))
    }

    /// A value an attribute can hold (Luau `typeof` decides which): bool, number, string, Vector2/3, Color3, CFrame
    /// and the number-only types. Anything else is an error that names the type.
    pub fn attribute_from_lua(&self, v: &Value) -> LuaResult<Option<DV>> {
        Ok(Some(match v {
            Value::Nil => return Ok(None),
            Value::Boolean(b) => DV::Bool(*b),
            Value::Number(n) => DV::Num(*n as f32),
            Value::Integer(i) => DV::Num(*i as f32),
            Value::String(s) => DV::Str(lstr(s)?),
            Value::Vector(x) => DV::V3(unvec(x)),
            Value::UserData(_) => match (as_color(v), as_cf(v), as_v2(v)) {
                (Some(c), _, _) => DV::Color(c.0),
                (_, Some(cf), _) => DV::CF(cf.p, cf.q),
                (_, _, Some(v2)) => DV::V2(v2),
                _ => return Err(rt("attribute values must be a boolean, number, string, Vector2, Vector3, Color3, CFrame, UDim, UDim2, Rect, NumberRange, NumberSequence, ColorSequence or BrickColor")),
            },
            Value::Table(t) => match table_type(t).as_deref().and_then(data_type) {
                Some(tag) => DV::Data(tag, self.encode_data(tag, v)?),
                None => return Err(rt("attribute values must be a boolean, number, string, Vector2, Vector3, Color3, CFrame, UDim, UDim2, Rect, NumberRange, NumberSequence, ColorSequence or BrickColor")),
            },
            other => return Err(rt(format!("attributes cannot hold a {}", other.type_name()))),
        }))
    }

    pub fn wrap(self: &Rc<Self>, lua: &Lua, id: Id) -> LuaResult<AnyUserData> {
        if let Some(ud) = self.cache.raw_get::<Option<AnyUserData>>(id.uid)? {
            return Ok(ud);
        }
        let ud = lua.create_userdata(InstanceRef { ctx: self.clone(), id })?;
        self.cache.raw_set(id.uid, ud.clone())?;
        Ok(ud)
    }

    /// Method results -> Lua values.
    pub fn script_to_lua(self: &Rc<Self>, lua: &Lua, v: ScriptValue) -> LuaResult<Value> {
        Ok(match v {
            ScriptValue::Nil => Value::Nil,
            ScriptValue::Bool(b) => Value::Boolean(b),
            ScriptValue::Num(n) => Value::Number(n),
            ScriptValue::Str(s) => Value::String(lua.create_string(&s)?),
            ScriptValue::Enum(s) => Value::String(lua.create_string(&s)?),
            ScriptValue::EnumItem(enum_name, item) => Value::Table(self.enums.raw_get::<Table>(enum_name)?.raw_get::<Table>(item.as_str())?),
            ScriptValue::Typed(tag, inner) => {
                let fields = self.script_to_lua(lua, *inner)?;
                self.set_type.call((tag, fields))?
            }
            ScriptValue::Vec2(v) => Value::UserData(lua.create_userdata(V2(v))?),
            ScriptValue::Vec3(x) => Value::Vector(vec(x)),
            ScriptValue::CFrame(p, q) => Value::UserData(lua.create_userdata(CF { p, q })?),
            ScriptValue::Color(c) => Value::UserData(lua.create_userdata(Color3(c))?),
            ScriptValue::Instance(id) => Value::UserData(self.wrap(lua, id)?),
            ScriptValue::Data(tag, numbers) => self.decode_data(lua, tag, &numbers)?,
            ScriptValue::List(items) => {
                let t = lua.create_table_with_capacity(items.len(), 0)?;
                // (By index, so a nil in the middle of a list leaves a hole instead of moving what follows.)
                for (i, it) in items.into_iter().enumerate() {
                    t.raw_set(i + 1, self.script_to_lua(lua, it)?)?;
                }
                Value::Table(t)
            }
            ScriptValue::Map(entries) => {
                let t = lua.create_table()?;
                for (k, val) in entries {
                    t.set(k, self.script_to_lua(lua, val)?)?;
                }
                Value::Table(t)
            }
            // (Only a method's results can be several values; one nested in another is just its first.)
            ScriptValue::Multi(items) => match items.into_iter().next() {
                Some(first) => self.script_to_lua(lua, first)?,
                None => Value::Nil,
            },
        })
    }

    /// What a method returns, as the values it hands back to the script.
    fn results_to_lua(self: &Rc<Self>, lua: &Lua, v: ScriptValue) -> LuaResult<MultiValue> {
        match v {
            ScriptValue::Multi(items) => items.into_iter().map(|item| self.script_to_lua(lua, item)).collect(),
            other => Ok(MultiValue::from_iter([self.script_to_lua(lua, other)?])),
        }
    }

    /// Property value -> Lua value.
    pub fn to_lua(self: &Rc<Self>, lua: &Lua, ty: PropType, v: &DV) -> LuaResult<Value> {
        Ok(match v {
            DV::Bool(b) => Value::Boolean(*b),
            DV::Num(n) => Value::Number(*n as f64),
            DV::Str(s) => Value::String(lua.create_string(s)?),
            DV::V2(v) => Value::UserData(lua.create_userdata(V2(*v))?),
            DV::V3(v) => Value::Vector(vec(*v)),
            DV::Color(c) => Value::UserData(lua.create_userdata(Color3(*c))?),
            DV::CF(p, q) => Value::UserData(lua.create_userdata(CF { p: *p, q: *q })?),
            DV::Enum(name) => match ty {
                PropType::Enum(t) => Value::Table(self.enums.raw_get::<Table>(t)?.raw_get::<Table>(name.as_str())?),
                _ => Value::String(lua.create_string(name)?),
            },
            DV::Ref(r) => match r {
                Some(id) if self.dm.borrow().exists(*id) => Value::UserData(self.wrap(lua, *id)?),
                _ => Value::Nil,
            },
            DV::Data(tag, numbers) => self.decode_data(lua, tag, numbers)?,
        })
    }

    /// Lua value -> property value of the given type.
    pub fn from_lua(&self, ty: PropType, v: &Value) -> LuaResult<DV> {
        match (ty, v) {
            (PropType::Bool, Value::Boolean(b)) => Ok(DV::Bool(*b)),
            (PropType::Number, Value::Number(n)) => Ok(DV::Num(*n as f32)),
            (PropType::Number, Value::Integer(i)) => Ok(DV::Num(*i as f32)),
            (PropType::String | PropType::ScriptPath | PropType::Asset(_), Value::String(s)) => Ok(DV::Str(lstr(s)?)),
            (PropType::Vector2, Value::UserData(_)) => as_v2(v).map(DV::V2).ok_or_else(|| rt("expected a Vector2")),
            (PropType::Vector3, Value::Vector(x)) => Ok(DV::V3(unvec(x))),
            (PropType::Color3, Value::UserData(_)) => as_color(v).map(|c| DV::Color(c.0)).ok_or_else(|| rt("expected a Color3")),
            (PropType::CFrame, Value::UserData(_)) => as_cf(v).map(|c| DV::CF(c.p, c.q)).ok_or_else(|| rt("expected a CFrame")),
            (PropType::Enum(t), Value::Table(tb)) => {
                let name: String = tb.raw_get("Name")?;
                // An item of another enum is refused even when an item of this one has the same name.
                let owner = tb.raw_get::<Option<Table>>("EnumType")?.and_then(|e| e.raw_get::<Option<String>>("__name").ok().flatten());
                if let Some(owner) = owner.filter(|o| o != t) {
                    return Err(rt(format!("expected an Enum.{t} item, got Enum.{owner}.{name}")));
                }
                if enum_names(t).contains(&name.as_str()) {
                    Ok(DV::Enum(name))
                } else {
                    Err(rt(format!("'{name}' is not a valid {t}")))
                }
            }
            (PropType::Enum(t), Value::String(s)) => {
                let name = lstr(s)?;
                if enum_names(t).contains(&name.as_str()) {
                    Ok(DV::Enum(name))
                } else {
                    Err(rt(format!("'{name}' is not a valid {t}")))
                }
            }
            (PropType::Ref(_), Value::Nil) => Ok(DV::Ref(None)),
            (PropType::Ref(want), Value::UserData(ud)) => {
                let r = ud.borrow::<InstanceRef>()?;
                let ok = want == "Instance" || self.dm.borrow().is_a(r.id, want);
                if ok {
                    Ok(DV::Ref(Some(r.id)))
                } else {
                    Err(rt(format!("expected a {want}")))
                }
            }
            // A number-only type: the script's value is turned into its numbers (an error says what was expected).
            // nil is accepted by the types that can be unset (`PhysicalProperties`).
            (PropType::Data(tag), Value::Table(_) | Value::Nil) => Ok(DV::Data(tag, self.encode_data(tag, v).map_err(|e| rt(format!("expected a {tag} ({})", e.to_string().lines().next().unwrap_or(""))))?)),
            _ => Err(rt(format!("invalid value ({}) for this property", v.type_name()))),
        }
    }

    pub fn signal(self: &Rc<Self>, id: Id, name: &str, hot: bool, touch: bool) -> LuaResult<Table> {
        if let Some(t) = self.existing_signal(id.uid, name) {
            return Ok(t);
        }
        let t: Table = self.signal_new.call(hot)?;
        self.signals.borrow_mut().entry(id.uid).or_default().insert(name.to_owned(), t.clone());
        let mut dm = self.dm.borrow_mut();
        dm.watch(id.uid);
        if touch {
            dm.watch_touch(id);
        }
        Ok(t)
    }

    pub fn existing_signal(&self, uid: u32, name: &str) -> Option<Table> {
        self.signals.borrow().get(&uid)?.get(name).cloned()
    }

    /// Whether any signal of the instance has been asked for (so an event for it can be skipped
    /// without building the names of its signals).
    pub fn has_signals(&self, uid: u32) -> bool {
        self.signals.borrow().contains_key(&uid)
    }

    pub fn fire0(&self, sig: &Table) -> LuaResult<()> {
        self.signal_fire.call::<()>(sig.clone())
    }

    pub fn fire1<A: mlua::IntoLua>(&self, sig: &Table, a: A) -> LuaResult<()> {
        self.signal_fire.call::<()>((sig.clone(), a))
    }

    pub fn fire2<A: mlua::IntoLua, B: mlua::IntoLua>(&self, sig: &Table, a: A, b: B) -> LuaResult<()> {
        self.signal_fire.call::<()>((sig.clone(), a, b))
    }

    /// The Lua function that runs a class-declared Rust method (created once per method).
    fn rust_method(self: &Rc<Self>, lua: &Lua, desc: &'static MethodDesc) -> LuaResult<Function> {
        let key = desc as *const MethodDesc as usize;
        if let Some(f) = self.method_cache.borrow().get(&key) {
            return Ok(f.clone());
        }
        let ctx = self.clone();
        let f = lua.create_function(move |lua, (target, args): (AnyUserData, Variadic<Value>)| {
            let t = this(&target)?;
            let mut script_args = Vec::with_capacity(args.len());
            for a in args.iter() {
                script_args.push(ctx.lua_to_script(a)?);
            }
            let result = {
                let mut dm = ctx.dm.borrow_mut();
                let mut physics = ctx.physics.borrow_mut();
                let mut call = MethodCall { dm: &mut *dm, this: t.id, args: script_args, host: &*ctx, physics: &mut *physics };
                (desc.func)(&mut call)
            };
            let value = result.map_err(rt)?;
            ctx.results_to_lua(lua, value)
        })?;
        self.method_cache.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// Methods implemented in Luau by class snippets: the class chain first, then the shared table.
    pub fn lua_method(&self, class: &'static ResolvedClass, key: &str) -> LuaResult<Option<Function>> {
        for c in &class.chain {
            if let Some(t) = self.class_methods.raw_get::<Option<Table>>(c.name)? {
                if let Some(f) = t.raw_get::<Option<Function>>(key)? {
                    return Ok(Some(f));
                }
            }
        }
        self.methods.raw_get::<Option<Function>>(key)
    }

    /// Compiles a script file with its own environment (`script` is defined in it).
    ///
    /// The chunk is named `path#uid`: the uid of the Script instance lets the runtime find out
    /// which script a function belongs to, so a destroyed script's connections can be dropped.
    pub fn load_chunk(&self, lua: &Lua, path: &str, script: AnyUserData) -> LuaResult<Function> {
        let uid = script.borrow::<InstanceRef>()?.id.uid;
        let name = format!("={path}#{uid}");
        let now = self.time.get();
        let cached = self.chunks.borrow().get(path).and_then(|c| {
            let unchanged = c.checked.get() == now || self.source.stamp(path).is_some_and(|s| s == c.stamp);
            if unchanged {
                c.checked.set(now);
            }
            unchanged.then(|| c.bytecode.clone())
        });
        let env: Table = self.new_env.call(script)?;
        let from_bytecode = |bytecode: &[u8]| lua.load(bytecode).set_mode(ChunkMode::Binary).set_name(name.clone()).set_environment(env.clone()).into_function();
        if let Some(bytecode) = cached {
            return from_bytecode(&bytecode);
        }
        let code = self.source.read_string(path).map_err(rt)?;
        // A script that does not compile takes the ordinary path, which reports the error exactly as
        // loading source always did (with the chunk name in front); only good scripts are cached.
        match (self.source.stamp(path), self.compiler.compile(&code)) {
            (Some(stamp), Ok(bytecode)) => {
                let function = from_bytecode(&bytecode)?;
                self.chunks.borrow_mut().insert(path.to_owned(), CompiledChunk { stamp, bytecode: bytecode.into(), checked: Cell::new(now) });
                Ok(function)
            }
            _ => {
                // What was compiled from an older version of the file must not be started again.
                self.chunks.borrow_mut().remove(path);
                lua.load(code).set_name(name).set_environment(env).into_function()
            }
        }
    }

    /// Forgets the signals of destroyed instances (their handlers could never fire again).
    pub fn forget_signals(&self, dead: &[u32]) {
        if dead.is_empty() {
            return;
        }
        let mut signals = self.signals.borrow_mut();
        for uid in dead {
            signals.remove(uid);
            let _ = self.callbacks.raw_remove(*uid);
        }
    }
}

impl InstanceRef {
    fn get_member(&self, lua: &Lua, key: &str) -> LuaResult<Value> {
        let ctx = &self.ctx;
        let (class, prop) = {
            let dm = ctx.dm.borrow();
            let Some(inst) = dm.get(self.id) else {
                // A destroyed instance still answers "no parent", so `while script.Parent do` loops end.
                return match key {
                    "Parent" => Ok(Value::Nil),
                    "Name" => Ok(Value::String(lua.create_string(ctx.dm.borrow().graves.get(&self.id.uid).map_or("", |g| g.1.as_str()))?)),
                    "ClassName" => ctx.dm.borrow().graves.get(&self.id.uid).map_or(Err(rt("this instance has been destroyed")), |g| Ok(Value::String(lua.create_string(g.0)?))),
                    // Taking away what is already gone is harmless in Roblox, too.
                    "Destroy" | "Remove" | "ClearAllChildren" => Ok(Value::Function(lua.create_function(|_, _: mlua::MultiValue| Ok(()))?)),
                    _ => Err(rt("this instance has been destroyed")),
                };
            };
            let class = inst.class;
            let prop = class.prop(key).and_then(|rp| dm.read_prop(inst, self.id, &rp.meta).map(|v| (rp.meta.ty, v)));
            (class, prop)
        };

        if let Some((ty, v)) = prop {
            return ctx.to_lua(lua, ty, &v);
        }
        if let Some(sig) = class.signal(key) {
            return Ok(Value::Table(ctx.signal(self.id, key, sig.hot, sig.touch)?));
        }
        if let Some(f) = self.method(lua, class, key)? {
            return Ok(Value::Function(f));
        }
        if class.is_callback(key) {
            let stored = ctx.callbacks.raw_get::<Option<Table>>(self.id.uid)?;
            return Ok(stored.map_or(Ok(Value::Nil), |t| t.raw_get::<Value>(key))?);
        }
        if class.is_unsupported(key) {
            let what = format!("{}:{key}", class.name);
            let refuse = lua.create_function(move |_, _: mlua::MultiValue| -> LuaResult<()> { Err(rt(crate::core::unsupported::<()>(&what).unwrap_err())) })?;
            return Ok(Value::Function(refuse));
        }
        let child = ctx.dm.borrow().find_child(self.id, key);
        if let Some(c) = child {
            return Ok(Value::UserData(ctx.wrap(lua, c)?));
        }
        // Roblox still accepts the old lower-case spellings of methods (`getChildren`, `findFirstChild`, `isA` ...).
        if let Some(first) = key.chars().next().filter(|c| c.is_ascii_lowercase()) {
            let modern = match key {
                "children" => "GetChildren".to_owned(),
                "remove" => "Remove".to_owned(),
                _ => format!("{}{}", first.to_ascii_uppercase(), &key[1..]),
            };
            if let Some(f) = self.method(lua, class, &modern)? {
                return Ok(Value::Function(f));
            }
        }
        let path = ctx.dm.borrow().full_name(self.id);
        Err(rt(format!("{key} is not a valid member of {} \"{path}\"", class.name)))
    }

    /// A method of the instance's class: one written in Rust, then one written in Luau.
    fn method(&self, lua: &Lua, class: &'static ResolvedClass, key: &str) -> LuaResult<Option<Function>> {
        if let Some(desc) = class.method(key) {
            return Ok(Some(self.ctx.rust_method(lua, desc)?));
        }
        self.ctx.lua_method(class, key)
    }

    fn set_member(&self, lua: &Lua, key: &str, val: Value) -> LuaResult<()> {
        let ctx = &self.ctx;
        let (class, uid) = ctx.dm.borrow().get(self.id).map(|i| (i.class, i.uid)).ok_or_else(|| rt("this instance has been destroyed"))?;
        let Some(rp) = class.prop(key) else {
            if class.is_callback(key) {
                return self.set_callback(lua, key, uid, val);
            }
            return Err(rt(format!("{key} is not a valid member of {}", class.name)));
        };
        if rp.meta.readonly {
            return Err(rt(format!("Unable to assign property {key}. Property is read only")));
        }
        let v = ctx.from_lua(rp.meta.ty, &val)?;
        let result = ctx.dm.borrow_mut().set_resolved(self.id, class, &rp.meta, v);
        result.map_err(rt)
    }

    /// Stores the function a script assigned to a callback member (`nil` removes it).
    fn set_callback(&self, lua: &Lua, key: &str, uid: u32, val: Value) -> LuaResult<()> {
        match val {
            Value::Function(_) | Value::Nil => {}
            other => return Err(rt(format!("{key} must be a function (got {})", other.type_name()))),
        }
        let callbacks = &self.ctx.callbacks;
        let table = match callbacks.raw_get::<Option<Table>>(uid)? {
            Some(t) => t,
            None => {
                let t = lua.create_table()?;
                callbacks.raw_set(uid, t.clone())?;
                // (So the callback is dropped when the instance is destroyed.)
                self.ctx.dm.borrow_mut().watch(uid);
                t
            }
        };
        table.raw_set(key, val)
    }
}

impl UserData for InstanceRef {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // (The key is read in place: copying it into a `String` would allocate on every property access.)
        m.add_meta_method(MetaMethod::Index, |lua, t, key: mlua::String| {
            let key = key.to_str()?;
            t.get_member(lua, &key)
        });
        m.add_meta_method(MetaMethod::NewIndex, |lua, t, (key, val): (mlua::String, Value)| {
            let key = key.to_str()?;
            t.set_member(lua, &key, val)
        });
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(t.ctx.dm.borrow().get(t.id).map(|i| i.name.clone()).unwrap_or_else(|| "<destroyed>".to_owned())));
    }
}

/// Globals that only need the context: `game`, `workspace`, `Instance`, `require`, `time`, ...
pub fn install_globals(lua: &Lua, ctx: &Rc<Ctx>) -> LuaResult<()> {
    let g = lua.globals();

    let (game, workspace) = {
        let dm = ctx.dm.borrow();
        (dm.game, dm.workspace)
    };
    g.set("game", ctx.wrap(lua, game)?)?;
    g.set("workspace", ctx.wrap(lua, workspace)?)?;

    let instance = lua.create_table()?;
    let c = ctx.clone();
    instance.set(
        "new",
        lua.create_function(move |lua, (class_name, parent): (mlua::String, Option<AnyUserData>)| {
            let name = class_name.to_str()?;
            let created = c.dm.borrow_mut().create(&name);
            let id = created.map_err(|_| rt(format!("Unable to create an Instance of type \"{}\"", &*name)))?;
            if let Some(p) = parent {
                let pid = p.borrow::<InstanceRef>()?.id;
                let result = c.dm.borrow_mut().set_parent(id, Some(pid));
                result.map_err(rt)?;
            }
            c.wrap(lua, id)
        })?,
    )?;
    // Instance.fromExisting(instance): a copy of the instance (and what is inside it).
    let c = ctx.clone();
    instance.set(
        "fromExisting",
        lua.create_function(move |lua, source: AnyUserData| {
            let id = source.borrow::<InstanceRef>()?.id;
            let copy = c.dm.borrow_mut().clone_tree(id).ok_or_else(|| rt("Unable to copy this Instance (it is not Archivable)"))?;
            c.wrap(lua, copy)
        })?,
    )?;
    g.set("Instance", instance)?;

    // Creates an instance of a class that scripts cannot create themselves (a Tween ...): for the Luau snippets of
    // the classes that hand such objects out.
    let c = ctx.clone();
    g.set(
        "__spawn",
        lua.create_function(move |lua, class_name: String| {
            let class = crate::core::class(&class_name).ok_or_else(|| rt(format!("'{class_name}' is not a valid class name")))?;
            let id = c.dm.borrow_mut().spawn_class(class);
            c.wrap(lua, id)
        })?,
    )?;

    // The function a script assigned to a callback member of an instance, or nil (for the classes' Luau snippets).
    let c = ctx.clone();
    g.set(
        "__callback",
        lua.create_function(move |_, (target, name): (AnyUserData, String)| {
            let uid = target.borrow::<InstanceRef>()?.id.uid;
            Ok(match c.callbacks.raw_get::<Option<Table>>(uid)? {
                Some(t) => t.raw_get::<Value>(name)?,
                None => Value::Nil,
            })
        })?,
    )?;

    // An error nobody caught reached the top of a script's thread: tell `ScriptContext.Error` (message, trace, script).
    let c = ctx.clone();
    g.set(
        "__script_error",
        lua.create_function(move |_, (message, trace, uid): (String, String, Option<u32>)| {
            let mut dm = c.dm.borrow_mut();
            let Some(service) = dm.service("ScriptContext") else { return Ok(()) };
            let script = uid.and_then(|uid| dm.ids_of_uids(&std::iter::once(uid).collect()).get(&uid).copied());
            dm.fire(service, "Error", || vec![ScriptValue::Str(message), ScriptValue::Str(trace), script.map_or(ScriptValue::Nil, ScriptValue::Instance)]);
            Ok(())
        })?,
    )?;

    // The signal called `name` of an instance, made on demand (for signals whose name depends on an argument: the
    // classes' Luau snippets hand these out, such as `CollectionService:GetInstanceAddedSignal(tag)`).
    g.set(
        "__signal",
        lua.create_function(|_, (target, name): (AnyUserData, String)| {
            let t = this(&target)?;
            t.ctx.signal(t.id, &name, false, false)
        })?,
    )?;

    // instance:GetPropertyChangedSignal("Name") - works on every class, so it lives in the shared table.
    ctx.methods.set(
        "GetPropertyChangedSignal",
        lua.create_function(|_, (target, name): (AnyUserData, String)| {
            let t = this(&target)?;
            t.ctx.signal(t.id, &format!("Changed:{name}"), false, false)
        })?,
    )?;
    // instance:GetAttributeChangedSignal("Name"): fires when that attribute is set, changed or removed.
    ctx.methods.set(
        "GetAttributeChangedSignal",
        lua.create_function(|_, (target, name): (AnyUserData, String)| {
            let t = this(&target)?;
            t.ctx.signal(t.id, &format!("AttributeChanged:{name}"), false, false)
        })?,
    )?;
    // instance:SetAttribute("Name", value): attributes can hold more kinds of value than a method argument can
    // carry, so the conversion is done here, where the types are known.
    ctx.methods.set(
        "SetAttribute",
        lua.create_function(|_, (target, name, value): (AnyUserData, String, Value)| {
            let t = this(&target)?;
            if let Some(problem) = DataModel::attribute_name_problem(&name) {
                return Err(rt(problem));
            }
            let value = t.ctx.attribute_from_lua(&value)?;
            if !t.ctx.dm.borrow().exists(t.id) {
                return Err(rt("this instance has been destroyed"));
            }
            t.ctx.dm.borrow_mut().set_attr(t.id, &name, value);
            Ok(())
        })?,
    )?;
    ctx.methods.set(
        "GetAttribute",
        lua.create_function(|lua, (target, name): (AnyUserData, String)| {
            let t = this(&target)?;
            let value = t.ctx.dm.borrow().attr(t.id, &name);
            match value {
                Some(v) => t.ctx.to_lua(lua, PropType::String, &v),
                None => Ok(Value::Nil),
            }
        })?,
    )?;
    ctx.methods.set(
        "GetAttributes",
        lua.create_function(|lua, target: AnyUserData| {
            let t = this(&target)?;
            let all = t.ctx.dm.borrow().attrs(t.id);
            let out = lua.create_table()?;
            for (name, v) in all {
                out.set(name, t.ctx.to_lua(lua, PropType::String, &v)?)?;
            }
            Ok(out)
        })?,
    )?;

    let c = ctx.clone();
    g.set(
        "__sched_wait",
        lua.create_function(move |_, (thread, secs, deferred): (Thread, f64, Option<bool>)| {
            let now = c.time.get();
            if deferred == Some(true) {
                c.sched.borrow_mut().defer(thread, now);
            } else {
                c.sched.borrow_mut().add(thread, now, secs);
            }
            Ok(())
        })?,
    )?;

    let c = ctx.clone();
    g.set("time", lua.create_function(move |_, ()| Ok(c.time.get()))?)?;
    g.set(
        "tick",
        lua.create_function(|_, ()| Ok(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)))?,
    )?;

    // What `require` (prelude.luau) needs from the host: the uid of the ModuleScript it was given (an error for
    // anything else), and, the first time, the module's compiled body.
    let c = ctx.clone();
    g.set(
        "__module_uid",
        lua.create_function(move |_, arg: Value| -> LuaResult<u32> {
            let Value::UserData(ud) = &arg else { return Err(rt("Attempted to call require with invalid argument(s).")) };
            let Ok(r) = ud.borrow::<InstanceRef>() else { return Err(rt("Attempted to call require with invalid argument(s).")) };
            let dm = c.dm.borrow();
            let inst = dm.get(r.id).ok_or_else(|| rt("The ModuleScript has been destroyed"))?;
            if !(inst.class.has_component("LuaSource") && !inst.class.has_component("ScriptRunner")) {
                return Err(rt(format!("Attempted to call require with invalid argument(s): a {} is not a ModuleScript", inst.class.name)));
            }
            Ok(inst.uid)
        })?,
    )?;
    let c = ctx.clone();
    g.set(
        "__module_chunk",
        lua.create_function(move |lua, ud: AnyUserData| -> LuaResult<Function> {
            let id = ud.borrow::<InstanceRef>()?.id;
            let path = c.dm.borrow().string(id, "SourceFile");
            if path.is_empty() {
                return Err(rt("the ModuleScript has no SourceFile"));
            }
            c.load_chunk(lua, &path, ud)
        })?,
    )?;

    Ok(())
}
