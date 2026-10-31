-- game:BindToClose(fn): fn runs when the game is closed by game:Shutdown() or a script kicking the player. It is
-- started, then the game closes: a handler that waits is not waited for (Roblox waits up to 30 seconds).
local handlers = {}

methods.BindToClose = function(_, fn)
	if type(fn) ~= "function" then
		error("BindToClose expects a function", 2)
	end
	table.insert(handlers, fn)
end

-- (Called by the host when the game is closing.)
define("__run_close", function()
	for _, fn in ipairs(table.clone(handlers)) do
		task.spawn(fn)
	end
	handlers = {}
end)