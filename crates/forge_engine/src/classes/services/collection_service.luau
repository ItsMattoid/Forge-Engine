-- The signals of a tag: they fire when an object is given the tag, and when it loses it (or is destroyed).
methods.GetInstanceAddedSignal = function(self, tag)
	return __signal(self, "TagAdded:" .. tag)
end

methods.GetInstanceRemovedSignal = function(self, tag)
	return __signal(self, "TagRemoved:" .. tag)
end
