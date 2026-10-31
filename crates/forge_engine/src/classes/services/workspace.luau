-- The older ray queries (FindPartOnRay and its variants): they return the part, the position it was hit at, the
-- surface normal and the material, and (nothing, the end of the ray, up, Air) when nothing was hit.
-- (RaycastParams and OverlapParams are in datatypes.luau.)
local function cast(workspace, ray, list, include)
	local params = RaycastParams.new()
	params.FilterDescendantsInstances = list
	params.FilterType = include and Enum.RaycastFilterType.Include or Enum.RaycastFilterType.Exclude
	local result = workspace:Raycast(ray.Origin, ray.Direction, params)
	if result then
		return result.Instance, result.Position, result.Normal, result.Material
	end
	return nil, ray.Origin + ray.Direction, Vector3.yAxis, Enum.Material.Air
end

methods.FindPartOnRay = function(self, ray, ignore)
	return cast(self, ray, ignore and { ignore } or {}, false)
end

methods.FindPartOnRayWithIgnoreList = function(self, ray, ignoreList)
	return cast(self, ray, ignoreList, false)
end

methods.FindPartOnRayWithWhitelist = function(self, ray, whitelist)
	return cast(self, ray, whitelist, true)
end

-- Whether any of the parts overlaps a part that is not in the list.
methods.ArePartsTouchingOthers = function(self, parts, overlap)
	local listed = {}
	for _, part in ipairs(parts) do
		listed[part] = true
	end
	for _, part in ipairs(parts) do
		for _, other in ipairs(self:GetPartsInPart(part)) do
			if not listed[other] then
				return true
			end
		end
	end
	return false
end

-- Puts each part at its CFrame.
methods.BulkMoveTo = function(self, parts, cframes)
	for i, part in ipairs(parts) do
		part.CFrame = cframes[i]
	end
end

-- Seconds since the epoch, as the server would count them.
methods.GetServerTimeNow = function()
	return os.time() + os.clock() % 1
end
