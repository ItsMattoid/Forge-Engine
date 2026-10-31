-- Nothing is for sale, so nobody owns anything.
methods.UserOwnsGamePassAsync = function()
	return false
end
methods.PlayerOwnsAsset = function()
	return false
end
methods.PlayerOwnsBundle = function()
	return false
end