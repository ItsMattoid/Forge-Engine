local topics = {} -- topic -> { connection, ... }

-- SubscribeAsync(topic, callback) -> a connection; the callback gets { Data = message, Sent = time }.
methods.SubscribeAsync = function(_, topic, callback)
	if type(topic) ~= "string" or type(callback) ~= "function" then
		error("SubscribeAsync(topic: string, callback: function)", 2)
	end
	local list = topics[topic]
	if not list then
		list = {}
		topics[topic] = list
	end
	local connection = { Connected = true }
	connection.callback = callback
	function connection:Disconnect()
		self.Connected = false
		local at = table.find(list, self)
		if at then
			table.remove(list, at)
		end
	end
	table.insert(list, connection)
	return setmetatable(connection, { __type = "RBXScriptConnection" })
end

methods.PublishAsync = function(_, topic, message)
	if type(topic) ~= "string" then
		error("PublishAsync(topic: string, message: any)", 2)
	end
	local packet = { Data = message, Sent = os.time() }
	for _, connection in ipairs(table.clone(topics[topic] or {})) do
		if connection.Connected then
			task.spawn(connection.callback, packet)
		end
	end
end