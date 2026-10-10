local config = require 'config'
local utils = require('utils')

local M = {}

function M.setup(opts)
  return config.merge(opts)
end

return M
