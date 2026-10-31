methods.GetUserInfosByUserIdsAsync = function(_, userIds)
	local player = game:GetService("Players").LocalPlayer
	local infos = {}
	for _, id in ipairs(userIds) do
		if id == player.UserId then
			table.insert(infos, { Id = id, Username = player.Name, DisplayName = player.DisplayName, HasVerifiedBadge = false })
		end
	end
	return infos
end