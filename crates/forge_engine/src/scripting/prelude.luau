-- Runs once, before the sandbox is sealed. Implements the generic parts of the
-- Roblox runtime that are easiest to express in Luau: Signals, the `task`
-- library, Enums, `require`, `typeof` and input objects. Everything class-specific lives in the class
-- files (Rust properties/methods, or `.luau` snippets next to them), and the data-only value types
-- (UDim2, NumberSequence, BrickColor ...) are in datatypes.luau.

local G = _G
local log = __log

local function join(...)
	local n = select("#", ...)
	local parts = table.create(n)
	for i = 1, n do
		parts[i] = tostring((select(i, ...)))
	end
	return table.concat(parts, " ")
end

function print(...)
	log(0, join(...))
end

function warn(...)
	log(1, join(...))
end

-- ---------------------------------------------------------------- typeof
-- Luau's own `typeof` reports the `__type` of a userdata's metatable (that is how the Rust types name themselves). A
-- table names itself the same way - a `__type` string in its metatable - and the native vector, which cannot carry a
-- metatable of its own, is the Vector3.
local builtin_typeof = typeof
function typeof(value)
	local t = builtin_typeof(value)
	if t == "vector" then
		return "Vector3"
	elseif t == "table" then
		local mt = getmetatable(value)
		local name = mt and rawget(mt, "__type")
		if type(name) == "string" then
			return name
		end
	end
	return t
end

-- ---------------------------------------------------------------- Enums
-- Enum.Material.Plastic is an EnumItem (Name, Value, EnumType); Enum.Material an Enum (its items, GetEnumItems,
-- FromName, FromValue); Enum itself holds the Enums (GetEnums). Names that Roblox renamed stay as aliases of the
-- item they became.
local Enum = {}
local enum_list = {}
local item_methods = {}
local item_mt = {
	__type = "EnumItem",
	__index = item_methods,
	__tostring = function(item)
		return "Enum." .. item.EnumType.__name .. "." .. item.Name
	end,
}

function item_methods.IsA(item, name)
	return name == "EnumItem" or name == item.EnumType.__name
end

local enum_methods = {}
function enum_methods.GetEnumItems(e)
	return table.clone(e.__list)
end
function enum_methods.FromName(e, name)
	return e.__by_name[name]
end
function enum_methods.FromValue(e, value)
	return e.__by_value[value]
end

local function define_enum(def)
	local e = { __name = def.name, __list = {}, __by_name = {}, __by_value = {} }
	for i, name in ipairs(def.items) do
		local value = def.numbers[i] or (i - 1)
		local item = setmetatable({ Name = name, Value = value, EnumType = e }, item_mt)
		e[name] = item
		e.__list[i] = item
		e.__by_name[name] = item
		e.__by_value[value] = item
	end
	for _, alias in ipairs(def.aliases) do
		local item = e.__by_name[alias[2]]
		if item then
			e[alias[1]] = item
			e.__by_name[alias[1]] = item
		end
	end
	Enum[def.name] = setmetatable(e, {
		__type = "Enum",
		__tostring = function()
			return "Enum." .. def.name
		end,
		__index = function(_, key)
			local method = enum_methods[key]
			if method then
				return method
			end
			error(tostring(key) .. " is not a valid member of Enum." .. def.name, 2)
		end,
	})
	table.insert(enum_list, Enum[def.name])
end

for _, def in ipairs(__enums) do
	define_enum(def)
end
__enums = nil

setmetatable(Enum, {
	__type = "Enums",
	__tostring = function()
		return "Enums"
	end,
	__index = function(_, key)
		if key == "GetEnums" then
			return function()
				return table.clone(enum_list)
			end
		end
		error(tostring(key) .. " is not a valid member of Enums", 2)
	end,
})

-- ---------------------------------------------------------------- script ownership
-- Every script's chunk is named "path#uid", so the uid of the Script instance a function was
-- written in can be read back from the function. Connections and threads started from a script
-- are remembered under that uid, and `reap(uid)` drops them when the script is destroyed,
-- disabled or moved out of the places scripts run in (a Roblox script's connections die with it).
local owned = {}
-- The script a thread belongs to (weak: a finished thread is forgotten).
local thread_owner = setmetatable({}, { __mode = "k" })

-- ---------------------------------------------------------------- profiling
-- Off unless the host asks (the debug overlay). While on, every handler, task and script chunk
-- is timed and its seconds are added to the script it belongs to: `cost[uid] = { seconds, calls }`.
-- Times are inclusive: a handler that spawns another counts the other one's time too.
local clock = os.clock
local profiling = false
local cost = {}

local function charge(uid, secs)
	local c = cost[uid]
	if c then
		c[1] += secs
		c[2] += 1
	else
		cost[uid] = { secs, 1 }
	end
end

local function owner_of(fn)
	if type(fn) ~= "function" then
		return nil
	end
	local src = debug.info(fn, "s")
	local uid = src and string.match(src, "#(%d+)$")
	return uid and tonumber(uid) or nil
end

local function remember(uid, kind, obj)
	local o = owned[uid]
	if not o then
		o = { conn = setmetatable({}, { __mode = "k" }), thread = setmetatable({}, { __mode = "k" }) }
		owned[uid] = o
	end
	o[kind][obj] = true
	if kind == "thread" then
		thread_owner[obj] = uid
	end
end

local function reap(uid, main)
	local o = owned[uid]
	owned[uid] = nil
	if o then
		for c in pairs(o.conn) do
			if c.Connected then
				c.Connected = false
				c._sig._dirty = true
			end
		end
		for co in pairs(o.thread) do
			if coroutine.status(co) == "suspended" then
				pcall(coroutine.close, co)
			end
		end
	end
	if main and coroutine.status(main) == "suspended" then
		pcall(coroutine.close, main)
	end
end

-- ---------------------------------------------------------------- task
local task = {}

-- Runs `f` in its own thread (so it may yield) and reports errors with a traceback.
local function spawn_owned(owner, f, ...)
	local co = type(f) == "thread" and f or coroutine.create(f)
	local ok, err
	if profiling then
		local started = clock()
		ok, err = coroutine.resume(co, ...)
		local uid = owner or thread_owner[co]
		if uid then
			charge(uid, clock() - started)
		end
	else
		ok, err = coroutine.resume(co, ...)
	end
	-- Only a thread that is still alive (it yielded: task.wait, :Wait() ...) has to be remembered, so
	-- that `reap` can close it with its script and later resumes are charged to that script. A handler
	-- that simply ran to the end -- nearly all of them -- never touches the (weak) bookkeeping tables.
	if owner and coroutine.status(co) == "suspended" then
		remember(owner, "thread", co)
	end
	if not ok then
		local trace = debug.traceback(co, tostring(err))
		log(2, trace)
		-- (ScriptContext.Error; the host provides `__script_error` once the game exists.)
		if __script_error then
			__script_error(tostring(err), trace, owner or thread_owner[co])
		end
	end
	return co
end

function task.spawn(f, ...)
	return spawn_owned(owner_of(f), f, ...)
end

function task.wait(t)
	local co = coroutine.running()
	__sched_wait(co, t or 0)
	return coroutine.yield()
end

function task.delay(t, f, ...)
	local args = table.pack(...)
	return spawn_owned(owner_of(f), function()
		task.wait(t)
		f(table.unpack(args, 1, args.n))
	end)
end

-- Runs `f` at the next resume point, before the threads that are waiting (as Roblox's deferred threads do).
function task.defer(f, ...)
	local args = table.pack(...)
	return spawn_owned(owner_of(f), function()
		__sched_wait(coroutine.running(), 0, true)
		coroutine.yield()
		f(table.unpack(args, 1, args.n))
	end)
end

function task.cancel(co)
	pcall(coroutine.close, co)
end

-- Scripts here run in one thread of execution: there is nothing to synchronize with.
function task.synchronize() end
function task.desynchronize() end

-- The older global functions. `wait` never returns sooner than about two frames, and `spawn` / `delay` start
-- their function on the next resume cycle, as Roblox's do.
function wait(t)
	local elapsed = task.wait(math.max(t or 0.03, 1 / 60))
	return elapsed, os.clock()
end

function spawn(f)
	return task.delay(0, f)
end

function delay(t, f)
	return task.delay(t or 0, f)
end

-- ---------------------------------------------------------------- Signals
local Signal = {}
Signal.__index = Signal
Signal.__type = "RBXScriptSignal"

local Connection = {}
Connection.__index = Connection
Connection.__type = "RBXScriptConnection"

function Connection:Disconnect()
	if self.Connected then
		self.Connected = false
		self._sig._dirty = true
	end
end
Connection.disconnect = Connection.Disconnect

function Signal.new(hot)
	return setmetatable({ _h = {}, _hot = hot, _dirty = false }, Signal)
end

local function connect(sig, fn, owner)
	local c = setmetatable({ Connected = true, _fn = fn, _sig = sig, _owner = owner }, Connection)
	table.insert(sig._h, c)
	if owner then
		remember(owner, "conn", c)
	end
	return c
end

function Signal:Connect(fn)
	if type(fn) ~= "function" then
		error("Attempt to connect failed: Passed value is not a function", 2)
	end
	return connect(self, fn, owner_of(fn))
end
Signal.connect = Signal.Connect
Signal.ConnectParallel = Signal.Connect

function Signal:Once(fn)
	if type(fn) ~= "function" then
		error("Attempt to connect failed: Passed value is not a function", 2)
	end
	local c
	c = connect(self, function(...)
		c:Disconnect()
		fn(...)
	end, owner_of(fn))
	return c
end

function Signal:Wait()
	local co = coroutine.running()
	self:Once(function(...)
		task.spawn(co, ...)
	end)
	return coroutine.yield()
end
Signal.wait = Signal.Wait

function Signal:Fire(...)
	local h = self._h
	local n = #h
	if n == 0 then
		return
	end
	-- Every handler runs in its own thread, so it may yield (task.wait, :Wait(), WaitForChild)
	-- no matter which signal fired.
	for i = 1, n do
		local c = h[i]
		if c and c.Connected then
			spawn_owned(c._owner, c._fn, ...)
		end
	end
	if self._dirty then
		self._dirty = false
		local j = 0
		for i = 1, #h do
			local c = h[i]
			if c.Connected then
				j += 1
				h[j] = c
			end
		end
		for i = #h, j + 1, -1 do
			h[i] = nil
		end
	end
end

-- ---------------------------------------------------------------- require
-- `require(moduleScript)` runs the module once and hands every later caller what it returned. It is written
-- here, not in Rust, so that a module may yield (task.wait ...) while it loads: a second script that requires it
-- meanwhile waits for it. The host says which uid a module has and gives its compiled body (`__module_uid`,
-- `__module_chunk`).
local modules = {}
local module_error = "Requested module experienced an error while loading"

function require(module)
	local uid = __module_uid(module)
	local entry = modules[uid]
	if entry == nil then
		entry = { thread = coroutine.running(), waiters = {} }
		modules[uid] = entry
		local chunk = __module_chunk(module)
		local result = table.pack(pcall(chunk))
		entry.thread = nil
		entry.done = true
		if not result[1] then
			entry.failed = true
			entry.message = result[2]
		elseif result.n ~= 2 then
			entry.failed = true
			entry.message = "Module code did not return exactly one value"
		else
			entry.value = result[2]
		end
		for _, waiting in ipairs(entry.waiters) do
			task.spawn(waiting)
		end
		entry.waiters = nil
		if entry.failed then
			error(entry.message, 0)
		end
		return entry.value
	end
	if not entry.done then
		if entry.thread == coroutine.running() then
			error("Requested module was required recursively (cyclic require)", 2)
		end
		table.insert(entry.waiters, coroutine.running())
		coroutine.yield()
	end
	if entry.failed then
		error(module_error, 2)
	end
	return entry.value
end

-- ---------------------------------------------------------------- Input objects
local input_mt = { __type = "InputObject" }
input_mt.__index = input_mt

local function mkinput(key, uit, state, dx, dy, dz)
	return setmetatable({
		KeyCode = key and Enum.KeyCode[key] or Enum.KeyCode.Unknown,
		UserInputType = Enum.UserInputType[uit],
		UserInputState = Enum.UserInputState[state],
		Delta = Vector3.new(dx, dy, dz),
		Position = Vector3.new(dx, dy, dz),
	}, input_mt)
end

-- ---------------------------------------------------------------- the rest of the global environment
local start = os.clock()
function elapsedTime()
	return os.clock() - start
end

function version()
	return "Forge 0.1 (Roblox Luau API)"
end

function ypcall(...)
	return pcall(...)
end

function printidentity()
	print("Current identity is 2")
end

function debug.profilebegin() end
function debug.profileend() end

-- A script's `_G` and `shared` are plain tables, shared by every script (the real globals are read-only once the
-- sandbox is sealed).
local user_G, user_shared = {}, {}

-- settings(), UserSettings() and stats(): the objects exist so that scripts reading a setting get a sensible
-- answer; none of them changes anything.
local global_settings, user_settings, stats_object

function settings()
	if not global_settings then
		local function section(fields)
			return fields
		end
		global_settings = {
			Rendering = section({ QualityLevel = Enum.QualityLevel.Automatic, MeshPartDetailLevel = Enum.MeshPartDetailLevel.DistanceBased }),
			Physics = section({ AllowSleep = true }),
			Network = section({}),
			Diagnostics = section({}),
			Studio = section({}),
			Game = section({}),
		}
	end
	return global_settings
end

function UserSettings()
	if not user_settings then
		local user_game_settings = { MasterVolume = 1, SavedQualityLevel = Enum.SavedQualityLevel.Automatic, MouseSensitivity = 1, ControlMode = Enum.ControlMode.MouseLockSwitch }
		user_settings = {
			GetService = function(_, name)
				if name == "UserGameSettings" then
					return user_game_settings
				end
				error("'" .. tostring(name) .. "' is not a valid UserSettings service", 2)
			end,
		}
	end
	return user_settings
end

function stats()
	stats_object = stats_object or {}
	return stats_object
end

-- ---------------------------------------------------------------- exports
G.Enum = Enum
G.task = task

return {
	signal_new = Signal.new,
	signal_fire = Signal.Fire,
	reap = reap,
	-- Forgets the modules that were loaded (the game was replaced).
	reset_modules = function()
		modules = {}
	end,
	-- Profiling (see above): switch it on or off, say which script a thread the host created belongs
	-- to, charge the host's own resumes to a script, and take (and reset) what was charged.
	set_profiling = function(on)
		profiling = on
		if not on then
			cost = {}
		end
	end,
	adopt = function(co, uid)
		thread_owner[co] = uid
	end,
	charge_thread = function(co, secs)
		local uid = thread_owner[co]
		if uid then
			charge(uid, secs)
		end
	end,
	charge_script = charge,
	take_costs = function()
		local taken = cost
		cost = {}
		return taken
	end,
	enums = Enum,
	mkinput = mkinput,
	-- Environment of a script: `script` is the Script instance.
	new_env = function(script)
		return setmetatable({ script = script, _G = user_G, shared = user_shared }, { __index = G })
	end,
	-- Environment of a class's Luau snippet (see ClassDesc::luau).
	snippet_env = function(methods)
		return setmetatable({
			methods = methods,
			Signal = Signal,
			task = task,
			Enum = Enum,
			define = function(name, value)
				G[name] = value
			end,
		}, { __index = G })
	end,
}
