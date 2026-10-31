-- event:Fire(...) calls everything connected to event.Event with the same arguments.
methods.Fire = function(self, ...)
	Signal.Fire(self.Event, ...)
end