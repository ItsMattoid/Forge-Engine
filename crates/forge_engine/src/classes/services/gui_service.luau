-- (top-left inset, bottom-right inset): nothing is drawn over the game.
methods.GetGuiInset = function()
	return Vector2.zero, Vector2.zero
end
methods.IsTenFootInterface = function()
	return false
end
methods.GetScreenResolution = function()
	return workspace.CurrentCamera.ViewportSize
end