defmodule PlugDemo do
  import Plug.Conn
  alias Logger
  use Plug.Router

  plug(:match)

  def call(conn, _opts) do
    send_resp(conn, 200, "ok")
  end
end
