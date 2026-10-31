methods.InvokeServer = function(self, ...)
	local callback = __callback(self, "OnServerInvoke")
	if not callback then
		error("InvokeServer: the RemoteFunction has no OnServerInvoke callback", 2)
	end
	return callback(game:GetService("Players").LocalPlayer, ...)
end

methods.InvokeClient = function(self, player, ...)
	if typeof(player) ~= "Instance" or not player:IsA("Player") then
		error("InvokeClient expects a Player as its first argument", 2)
	end
	local callback = __callback(self, "OnClientInvoke")
	if not callback then
		error("InvokeClient: the RemoteFunction has no OnClientInvoke callback", 2)
	end
	return callback(...)
end