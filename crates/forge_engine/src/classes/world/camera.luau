-- Maps between the world and the picture the camera draws. The picture is `ViewportSize` pixels with (0, 0) at the
-- top left; depth is the distance in front of the camera. (The same pinhole camera the renderer uses: FieldOfView is
-- vertical.)

-- The world point at pixel (x, y) and `depth` studs in front of the camera, as a Ray from there along the line of
-- sight through the pixel.
methods.ViewportPointToRay = function(self, x, y, depth)
	local size = self.ViewportSize
	local height = 2 * math.tan(math.rad(self.FieldOfView) / 2)
	local width = height * size.X / size.Y
	local cf = self.CFrame
	local dx, dy = x / size.X - 0.5, 0.5 - y / size.Y
	local direction = (cf.RightVector * (dx * width) + cf.UpVector * (dy * height) + cf.LookVector).Unit
	return Ray.new(cf.Position + direction * (depth or 0), direction)
end
methods.ScreenPointToRay = methods.ViewportPointToRay

-- (pixel x, pixel y, depth) of a world point, and whether it is inside the picture and in front of the camera.
local function project(self, position)
	local size = self.ViewportSize
	local height = 2 * math.tan(math.rad(self.FieldOfView) / 2)
	local width = height * size.X / size.Y
	local cf = self.CFrame
	local local_point = cf:PointToObjectSpace(position)
	local depth = -local_point.Z
	local x, y = 0, 0
	if depth ~= 0 then
		x = (local_point.X / depth / width + 0.5) * size.X
		y = (0.5 - local_point.Y / depth / height) * size.Y
	end
	local on_screen = depth > 0 and x >= 0 and x <= size.X and y >= 0 and y <= size.Y
	return Vector3.new(x, y, depth), on_screen
end
methods.WorldToViewportPoint = project
methods.WorldToScreenPoint = project

-- The camera's own Roblox helpers for the size of things in view.
methods.GetPartsObscuringTarget = function(self, targets, ignore)
	local parts = {}
	local params = RaycastParams.new()
	params.FilterDescendantsInstances = ignore or {}
	for _, target in ipairs(targets) do
		local origin = self.CFrame.Position
		local result = workspace:Raycast(origin, target - origin, params)
		if result then
			table.insert(parts, result.Instance)
		end
	end
	return parts
end