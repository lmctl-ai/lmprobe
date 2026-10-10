#pragma once

#include <vector>

namespace util {

template <typename T>
class Stack {
public:
    void push(T value) { data_.push_back(value); }
    T pop();
    bool empty() const { return data_.empty(); }

private:
    std::vector<T> data_;
};

template <typename T>
T Stack<T>::pop() {
    T value = data_.back();
    data_.pop_back();
    return value;
}

using IntStack = Stack<int>;

}  // namespace util
