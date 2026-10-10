require 'sinatra'
require_relative '../config/settings'

class Application < Sinatra::Base
  get '/' do
    'hello'
  end
end
