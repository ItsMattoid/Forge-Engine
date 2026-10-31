-- A store is a table of key -> JSON text (so what is read is always a copy, and what cannot be stored is refused when it is
-- written, as in Roblox). The stores of the game live here until it ends.
local databases = {} -- "name/scope" -> { name, scope, data = { key -> text } }

local function http()
	return game:GetService("HttpService")
end

local function encode(value)
	local ok, text = pcall(function()
		return http():JSONEncode(value)
	end)
	if not ok then
		error("104: Cannot store " .. typeof(value) .. " in data store. Data stores can only accept valid UTF-8 characters.", 3)
	end
	if #text > 4194304 then
		error("105: Serialized value exceeds 4MB limit.", 3)
	end
	return text
end

local function decode(text)
	if text == nil then
		return nil
	end
	return http():JSONDecode(text)
end

local function check_key(key)
	if type(key) ~= "string" then
		error("bad argument #1 to 'key': string expected, got " .. typeof(key), 3)
	end
	if key == "" then
		error("102: Key name can't be empty.", 3)
	end
	if #key > 50 then
		error("101: Key name exceeds 50 character limit.", 3)
	end
end

local DataStore = {}
DataStore.__index = DataStore
DataStore.__type = "DataStore"

function DataStore:GetAsync(key)
	check_key(key)
	return decode(self._db.data[key])
end

function DataStore:SetAsync(key, value)
	check_key(key)
	if value == nil then
		self._db.data[key] = nil
	else
		self._db.data[key] = encode(value)
	end
	self._db.version[key] = (self._db.version[key] or 0) + 1
	return tostring(self._db.version[key])
end

function DataStore:UpdateAsync(key, transform)
	check_key(key)
	local result = transform(decode(self._db.data[key]))
	if result == nil then
		return nil -- the update was cancelled
	end
	self._db.data[key] = encode(result)
	self._db.version[key] = (self._db.version[key] or 0) + 1
	return decode(self._db.data[key])
end

function DataStore:IncrementAsync(key, delta)
	check_key(key)
	local old = decode(self._db.data[key]) or 0
	if type(old) ~= "number" then
		error("The value of '" .. key .. "' is not a number and cannot be incremented.", 2)
	end
	local new = old + (delta or 1)
	self._db.data[key] = encode(new)
	return new
end

function DataStore:RemoveAsync(key)
	check_key(key)
	local old = decode(self._db.data[key])
	self._db.data[key] = nil
	return old
end

function DataStore:GetVersionAsync(key)
	error("DataStore:GetVersionAsync is not supported by Forge (it keeps no history of a key's values)", 2)
end

function DataStore:ListKeysAsync()
	error("DataStore:ListKeysAsync is not supported by Forge", 2)
end

-- Ordered data stores keep whole numbers and can be listed from the smallest or the largest.
local OrderedDataStore = setmetatable({}, { __index = DataStore })
OrderedDataStore.__index = OrderedDataStore
OrderedDataStore.__type = "OrderedDataStore"

function OrderedDataStore:SetAsync(key, value)
	if type(value) ~= "number" or value % 1 ~= 0 then
		error("Can't store " .. typeof(value) .. " in OrderedDataStore: only integers", 2)
	end
	return DataStore.SetAsync(self, key, value)
end

function OrderedDataStore:GetSortedAsync(ascending, pageSize, minValue, maxValue)
	local entries = {}
	for key, text in pairs(self._db.data) do
		local value = decode(text)
		if (not minValue or value >= minValue) and (not maxValue or value <= maxValue) then
			table.insert(entries, { key = key, value = value })
		end
	end
	table.sort(entries, function(a, b)
		if a.value == b.value then
			return a.key < b.key
		end
		if ascending then
			return a.value < b.value
		end
		return a.value > b.value
	end)
	local pages = { IsFinished = false }
	local first = 1
	local function page()
		local current = {}
		for i = first, math.min(first + pageSize - 1, #entries) do
			table.insert(current, entries[i])
		end
		pages.IsFinished = first + pageSize > #entries
		return current
	end
	local current = page()
	function pages:GetCurrentPage()
		return current
	end
	function pages:AdvanceToNextPageAsync()
		if not pages.IsFinished then
			first += pageSize
			current = page()
		end
	end
	return setmetatable(pages, { __type = "DataStorePages" })
end

local function open(self_service, class, name, scope)
	if type(name) ~= "string" or name == "" then
		error("DataStore name can't be empty.", 3)
	end
	scope = scope or "global"
	local id = class.__type .. "/" .. name .. "/" .. scope
	local db = databases[id]
	if not db then
		db = { data = {}, version = {} }
		databases[id] = db
	end
	return setmetatable({ _db = db, Name = name }, class)
end

methods.GetDataStore = function(self, name, scope)
	return open(self, DataStore, name, scope)
end

methods.GetGlobalDataStore = function(self)
	return open(self, DataStore, "global", "global")
end

methods.GetOrderedDataStore = function(self, name, scope)
	return open(self, OrderedDataStore, name, scope)
end

methods.GetRequestBudgetForRequestType = function()
	return 60
end