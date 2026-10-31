methods.GetTotalMemoryUsageMb = function()
	return collectgarbage("count") / 1024
end
methods.GetMemoryUsageMbForTag = function()
	return 0
end