-- Methods of every Instance that are easiest to write in Luau (they yield).

-- Waits until a child with this name exists (optionally giving up after `timeout` seconds).
methods.WaitForChild = function(self, name, timeout)
	local found = self:FindFirstChild(name)
	if found then
		return found
	end
	local co = coroutine.running()
	local done = false
	local conn
	conn = self.ChildAdded:Connect(function(child)
		if not done and child.Name == name then
			done = true
			conn:Disconnect()
			task.spawn(co, child)
		end
	end)
	if timeout then
		task.delay(timeout, function()
			if not done then
				done = true
				conn:Disconnect()
				task.spawn(co, nil)
			end
		end)
	end
	return coroutine.yield()
end
