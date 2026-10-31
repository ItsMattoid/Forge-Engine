-- GetTextSize(text, fontSize, font, frameSize): the size the text takes when wrapped to the frame's width.
methods.GetTextSize = function(_, text, fontSize, _font, frameSize)
	local charWidth = fontSize * 0.5
	local lines = 0
	local widest = 0
	for line in string.gmatch(text .. "\n", "(.-)\n") do
		local width = #line * charWidth
		local wrapped = math.max(1, math.ceil(width / math.max(frameSize.X, charWidth)))
		lines += wrapped
		widest = math.max(widest, math.min(width, frameSize.X))
	end
	return Vector2.new(widest, lines * fontSize)
end

-- FilterStringAsync(text, fromUserId, context?) -> a result whose Get...Async methods return the text.
local result_mt = { __type = "TextFilterResult" }
result_mt.__index = result_mt
function result_mt:GetNonChatStringForBroadcastAsync()
	return self._text
end
function result_mt:GetNonChatStringForUserAsync()
	return self._text
end
function result_mt:GetChatForUserAsync()
	return self._text
end
methods.FilterStringAsync = function(_, text)
	return setmetatable({ _text = text }, result_mt)
end