#include <stdlib.h>

struct Node {
    int value;
    struct Node *next;
};

struct Node *create_node(int val) {
    struct Node *n = malloc(sizeof(struct Node));
    n->value = val;
    n->next = NULL;
    return n;
}

void push(struct Node **head, int val) {
    struct Node *n = create_node(val);
    n->next = *head;
    *head = n;
}

int length(struct Node *head) {
    int count = 0;
    while (head) {
        count++;
        head = head->next;
    }
    return count;
}
