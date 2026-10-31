local function local_player()
	return game:GetService("Players").LocalPlayer
end

methods.FireServer = function(self, ...)
	Signal.Fire(self.OnServerEvent, local_player(), ...)
end

methods.FireClient = function(self, player, ...)
	if typeof(player) ~= "Instance" or not player:IsA("Player") then
		error("FireClient expects a Player as its first argument", 2)
	end
	Signal.Fire(self.OnClientEvent, ...)
end

methods.FireAllClients = function(self, ...)
	Signal.Fire(self.OnClientEvent, ...)
end