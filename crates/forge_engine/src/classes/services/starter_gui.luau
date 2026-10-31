-- Roblox's own interface (the player list, the health bar ...) does not exist here, so switching it is accepted and
-- ignored, and every part reports as enabled.
methods.SetCoreGuiEnabled = function() end
methods.GetCoreGuiEnabled = function()
	return true
end
methods.SetCore = function() end
methods.GetCore = function()
	return nil
end
methods.RegisterSetCore = function() end
methods.RegisterGetCore = function() end