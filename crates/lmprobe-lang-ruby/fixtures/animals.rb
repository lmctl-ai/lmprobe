class Animal
  def speak
    raise NotImplementedError
  end
end

class Dog < Animal
  def speak
    "Woof!"
  end
end

class Cat < Animal
  def speak
    "Meow!"
  end

  def self.lives
    9
  end
end
