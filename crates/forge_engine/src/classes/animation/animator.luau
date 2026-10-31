-- An AnimationTrack: the state and signals of one loaded animation. Nothing is posed.
local playing_of = setmetatable({}, { __mode = "k" }) -- Animator -> the tracks playing on it

local track_mt = { __type = "AnimationTrack" }
track_mt.__index = track_mt

local function signal()
	return Signal.new(false)
end

function track_mt:Play(fadeTime, weight, speed)
	if self.IsPlaying then
		return
	end
	self.IsPlaying = true
	self.TimePosition = 0
	self.Speed = speed or self.Speed
	self.WeightTarget = weight or 1
	self.WeightCurrent = self.WeightTarget
	local playing = playing_of[self._animator]
	if not playing then
		playing = {}
		playing_of[self._animator] = playing
	end
	table.insert(playing, self)
	Signal.Fire(self._animator.AnimationPlayed, self)
	-- No animation has a length here, so one that does not loop is over at once.
	if not self.Looped then
		task.defer(function()
			self:Stop()
		end)
	end
end

function track_mt:Stop()
	if not self.IsPlaying then
		return
	end
	self.IsPlaying = false
	local playing = playing_of[self._animator]
	local at = playing and table.find(playing, self)
	if at then
		table.remove(playing, at)
	end
	Signal.Fire(self.Stopped)
	Signal.Fire(self.Ended)
end

function track_mt:AdjustSpeed(speed)
	self.Speed = speed
end

function track_mt:AdjustWeight(weight)
	self.WeightTarget = weight
	self.WeightCurrent = weight
end

function track_mt:GetTimeOfKeyframe()
	return 0
end

function track_mt:GetMarkerReachedSignal()
	return signal()
end

function track_mt:Destroy()
	self:Stop()
end

methods.LoadAnimation = function(self, animation)
	if typeof(animation) ~= "Instance" or not animation:IsA("Animation") then
		error("LoadAnimation expects an Animation", 2)
	end
	return setmetatable({
		Animation = animation,
		Name = animation.Name,
		IsPlaying = false,
		Length = 0,
		Looped = false,
		Priority = Enum.AnimationPriority.Core,
		Speed = 1,
		TimePosition = 0,
		WeightCurrent = 1,
		WeightTarget = 1,
		Stopped = signal(),
		Ended = signal(),
		DidLoop = signal(),
		KeyframeReached = signal(),
		_animator = self,
	}, track_mt)
end

methods.GetPlayingAnimationTracks = function(self)
	return table.clone(playing_of[self] or {})
end