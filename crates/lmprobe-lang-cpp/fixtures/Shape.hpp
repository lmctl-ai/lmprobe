#pragma once

#include <string>

namespace geom {

class Shape {
public:
    virtual ~Shape() = default;
    virtual double area() const = 0;
    virtual std::string name() const { return "shape"; }
};

class Circle : public Shape {
public:
    explicit Circle(double r) : radius_(r) {}
    double area() const override;
    std::string name() const override { return "circle"; }

private:
    double radius_;
};

struct Point {
    double x;
    double y;
};

}  // namespace geom
