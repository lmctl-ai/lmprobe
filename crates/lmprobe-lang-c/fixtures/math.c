#include <stdio.h>
#include <math.h>

#define PI 3.14159
#define SQUARE(x) ((x) * (x))

typedef struct {
    double x;
    double y;
} Point;

struct Circle {
    Point center;
    double radius;
};

enum Color {
    RED,
    GREEN,
    BLUE
};

double area(struct Circle *c) {
    return PI * SQUARE(c->radius);
}

int *get_pointer(int *arr, int idx) {
    return &arr[idx];
}

static void print_point(Point p) {
    printf("(%f, %f)\n", p.x, p.y);
}
