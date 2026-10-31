-- PreloadAsync(instances, callback?): nothing needs loading ahead; the callback hears of each asset as loaded.
methods.PreloadAsync = function(_, instances, callback)
	if callback then
		for _, item in ipairs(instances) do
			callback(tostring(item), Enum.AssetFetchStatus.Success)
		end
	end
end