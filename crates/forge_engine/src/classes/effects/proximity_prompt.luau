-- A prompt is used by holding its key: InputHoldBegin starts a hold, InputHoldEnd lets go. The prompt triggers once the key
-- has been held for HoldDuration (at once when that is 0). The keyboard (UserInputService) calls these for the local
-- player when the prompt's key goes down near it; a script can call them itself, as Roblox's own prompt interface does.

local holding = setmetatable({}, { __mode = "k" }) -- prompt -> the hold in progress

local function service()
	return game:GetService("ProximityPromptService")
end

methods.InputHoldBegin = function(self)
	if holding[self] or not self.Enabled or not service().Enabled then
		return
	end
	local player = game:GetService("Players").LocalPlayer
	local hold = { triggered = false }
	holding[self] = hold
	local function trigger()
		hold.triggered = true
		Signal.Fire(self.Triggered, player)
		Signal.Fire(service().PromptTriggered, self, player)
	end
	if self.HoldDuration > 0 then
		Signal.Fire(self.PromptButtonHoldBegan, player)
		Signal.Fire(service().PromptButtonHoldBegan, self, player)
		task.delay(self.HoldDuration, function()
			if holding[self] == hold then
				trigger()
			end
		end)
	else
		trigger()
	end
end

methods.InputHoldEnd = function(self)
	local hold = holding[self]
	if not hold then
		return
	end
	holding[self] = nil
	local player = game:GetService("Players").LocalPlayer
	if self.HoldDuration > 0 then
		Signal.Fire(self.PromptButtonHoldEnded, player)
		Signal.Fire(service().PromptButtonHoldEnded, self, player)
	end
	if hold.triggered then
		Signal.Fire(self.TriggerEnded, player)
		Signal.Fire(service().PromptTriggerEnded, self, player)
	end
end
