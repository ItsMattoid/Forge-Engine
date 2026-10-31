methods.AddItem = function(self, item, life)
	task.delay(life or 10, function()
		if item.Parent then
			item:Destroy()
		end
	end)
end
