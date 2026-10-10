require 'json'
require_relative 'helpers'

module Greetable
  def greet(name)
    "Hello, #{name}!"
  end
end

class Greeter
  include Greetable

  def initialize(prefix)
    @prefix = prefix
  end

  def say(name)
    "#{@prefix} #{greet(name)}"
  end

  def self.default
    new("Hi")
  end
end
