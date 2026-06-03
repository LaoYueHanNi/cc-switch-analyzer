import { createRouter, createWebHashHistory } from 'vue-router'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      redirect: '/by-model'
    },
    {
      path: '/by-model',
      name: 'by-model',
      component: () => import('@/views/ByModel.vue')
    },
    {
      path: '/by-provider',
      name: 'by-provider',
      component: () => import('@/views/ByProvider.vue')
    },
    {
      path: '/trend',
      name: 'trend',
      component: () => import('@/views/TrendAnalysis.vue')
    },
    {
      path: '/session',
      name: 'session',
      component: () => import('@/views/SessionAnalysis.vue')
    },
    {
      path: '/realtime',
      name: 'realtime',
      component: () => import('@/views/RealtimeToken.vue')
    },
    {
      path: '/pricing',
      name: 'pricing',
      component: () => import('@/views/PricingCalculator.vue')
    },
    {
      path: '/task',
      name: 'task',
      component: () => import('@/views/Task.vue')
    },
    {
      path: '/task/:id',
      name: 'task-detail',
      component: () => import('@/views/TaskDetail.vue')
    }
  ]
})

export default router
