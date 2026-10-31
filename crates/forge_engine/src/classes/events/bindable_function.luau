methods.Invoke = function(self, ...)
	local callback = __callback(self, "OnInvoke")
	if not callback then
		error("Attempt to invoke a BindableFunction that has no OnInvoke callback", 2)
	end
	return callback(...)
end