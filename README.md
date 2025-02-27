TODO: structure

Ring's are Vec's of resources, typically in count of 2, to simplify synchronization with CPU
they are accessed by handles, not directly, to manage them (move, free, recreate, etc) automatically
it also makes referencing them easier (which defeats borrow checker lol)