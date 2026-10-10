package fixture

func first() {
	wrongCall()
}

func second() {
	rightCall()
}

func wrongCall() {}

func rightCall() {}
